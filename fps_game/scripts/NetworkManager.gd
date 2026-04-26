extends Node

@export var use_cloud := true

const CLOUD_IP := "3.218.9.34"
const LOCAL_IP := "127.0.0.1"
const TOKYO_IP := "57.181.105.56"

enum Server { CLOUD, LOCAL, TOKYO }

var active_server := Server.CLOUD

# server ip based on active server
var server_ip: String:
	get: 
		match active_server:
			Server.CLOUD: return CLOUD_IP
			Server.LOCAL: return LOCAL_IP
			Server.TOKYO: return TOKYO_IP
			_: return LOCAL_IP

var tcp_port := 7777
var udp_port_server := 7778

# state
var my_player_id: int = -1
var input_seq: int = 0
var simulate_local := false

# sockets
var tcp: StreamPeerTCP
var udp: PacketPeerUDP
var my_udp_port: int = 0

# scene refs
var local_player: Node = null
var remote_players: Dictionary = {}  # player_id (int) -> RemotePlayer node

var _drift_log: Array = []
var _start_time: float = 0.0

signal player_joined(player_id: int)

func _ready():
	_start_time = Time.get_ticks_msec() / 1000.0

func connect_to_server():
	_start_udp()
	_connect_tcp()

# bind udp first so we know our local port before tcp handshake
func _start_udp():
	udp = PacketPeerUDP.new()
	udp.bind(0)
	my_udp_port = udp.get_local_port()
	udp.set_dest_address(server_ip, udp_port_server)
	print("UDP bound on local port: ", my_udp_port)

# tcp connect then send connect packet
func _connect_tcp():
	print("Connecting to: ", server_ip, ":", tcp_port)
	tcp = StreamPeerTCP.new()
	tcp.set_no_delay(true)
	tcp.connect_to_host(server_ip, tcp_port)
	await _wait_tcp_connected()
	_send_connect()

# wait for tcp status to leave connecting, timeout after 1 second
func _wait_tcp_connected():
	var timeout := 1.0
	var elapsed := 0.0
	while tcp.get_status() == StreamPeerTCP.STATUS_CONNECTING:
		tcp.poll()
		await get_tree().process_frame
		elapsed += get_process_delta_time()
		if elapsed >= timeout:
			push_error("TCP connection timed out")
			return
	if tcp.get_status() != StreamPeerTCP.STATUS_CONNECTED:
		push_error("TCP connection failed")

# send connect packet (0x01) with our udp port
func _send_connect():
	var buf := PackedByteArray()
	buf.append(0x01)
	buf.append((my_udp_port >> 8) & 0xFF)
	buf.append(my_udp_port & 0xFF)
	tcp.put_data(buf)
	print("Sent Connect (0x01) with UDP port ", my_udp_port)

# frame loop - poll tcp and udp every frame once connected
func _process(_delta):
	_poll_tcp()
	if my_player_id >= 0:
		_send_player_input()
		_poll_udp()

# tcp in: read connected (0x10) and store assigned player id
func _poll_tcp():
	if tcp == null:
		return
	tcp.poll()
	if tcp.get_status() != StreamPeerTCP.STATUS_CONNECTED:
		return
	if tcp.get_available_bytes() < 3:
		return
	var type_byte = tcp.get_u8()
	if type_byte == 0x10:
		var hi = tcp.get_u8()
		var lo = tcp.get_u8()
		my_player_id = (hi << 8) | lo
		print("Connected! player_id = ", my_player_id)

# udp out: send player input (0x02) every frame
# layout: [type:u8, seq:u16be, yaw:f32be, pitch:f32be, move_x:i8, move_z:i8]
func _send_player_input():
	if udp == null or local_player == null:
		return
	var move_x: int = 0
	var move_z: int = 0
	if Input.is_action_pressed("move_right"):    move_x += 1
	if Input.is_action_pressed("move_left"):     move_x -= 1
	if Input.is_action_pressed("move_backward"): move_z += 1
	if Input.is_action_pressed("move_forward"):  move_z -= 1
	var yaw: float = local_player.rotation.y
	var pitch: float = 0.0
	if local_player.has_node("CameraMount"):
		pitch = local_player.get_node("CameraMount").rotation.x
	input_seq = (input_seq + 1) % 65536
	var buf := PackedByteArray()
	buf.append(0x02)
	buf.append_array(_pack_u16(input_seq))
	buf.append_array(_pack_f32_be(yaw))
	buf.append_array(_pack_f32_be(pitch))
	buf.append(_i8_to_u8(move_x))
	buf.append(_i8_to_u8(move_z))
	udp.put_packet(buf)

# udp in: route incoming packets by type
func _poll_udp():
	if udp == null:
		return
	while udp.get_available_packet_count() > 0:
		var packet = udp.get_packet()
		if packet.size() < 1:
			continue
		match packet[0]:
			0x11:
				_handle_world_state(packet)
			0x12:
				_handle_player_left(packet)

# udp in: parse world state and apply positions to remote players
# layout: [type:u8, count:u8, then count x 22 bytes per player]
func _handle_world_state(packet: PackedByteArray):
	var player_count = packet[1]
	var expected = 2 + player_count * 22
	if packet.size() != expected:
		push_warning("WorldState size mismatch: got %d, expected %d" % [packet.size(), expected])
		return
	var offset = 2
	for i in range(player_count):
		var pid   = _unpack_u16(packet, offset);    offset += 2
		var px    = _unpack_f32_be(packet, offset); offset += 4
		var py    = _unpack_f32_be(packet, offset); offset += 4
		var pz    = _unpack_f32_be(packet, offset); offset += 4
		var yaw   = _unpack_f32_be(packet, offset); offset += 4
		var pitch = _unpack_f32_be(packet, offset); offset += 4
		if pid == my_player_id:
			_reconcile_local(Vector3(px, py, pz))
		else:
			_apply_remote(pid, Vector3(px, py, pz), yaw)

# udp in: despawn remote player node on disconnect (0x12)
# layout: [type:u8, player_id:u16be]
func _handle_player_left(packet: PackedByteArray):
	if packet.size() < 3:
		push_warning("PlayerLeft packet too short")
		return
	var pid = _unpack_u16(packet, 1)
	if remote_players.has(pid):
		var node = remote_players[pid]
		if is_instance_valid(node):
			node.queue_free()
		remote_players.erase(pid)
		print("[-] Player ", pid, " left, node despawned")

# reconcile local player position against server, log drift
func _reconcile_local(server_pos: Vector3):
	if local_player == null:
		return
	var drift = server_pos.distance_to(local_player.global_position)
	var elapsed = (Time.get_ticks_msec() / 1000.0) - _start_time
	_drift_log.append({"time": elapsed, "drift": drift})
	if drift > 0.5:
		local_player.global_position = local_player.global_position.lerp(server_pos, 0.3)

# apply server position and yaw to a remote player node
func _apply_remote(pid: int, pos: Vector3, yaw: float):
	if not remote_players.has(pid):
		emit_signal("player_joined", pid)
		return
	var rp = remote_players[pid]
	if is_instance_valid(rp):
		rp.apply_state(pos, yaw)

# called by LocalPlayer.gd and Main.gd after spawning nodes
func register_local_player(node: Node):
	local_player = node

func register_remote_player(pid: int, node: Node):
	remote_players[pid] = node

# byte helpers - all big-endian to match Rust's to_be_bytes()

func _pack_u16(val: int) -> PackedByteArray:
	var b := PackedByteArray()
	b.append((val >> 8) & 0xFF)
	b.append(val & 0xFF)
	return b

func _unpack_u16(buf: PackedByteArray, offset: int) -> int:
	return (buf[offset] << 8) | buf[offset + 1]

func _pack_f32_be(val: float) -> PackedByteArray:
	# encode as LE first then reverse to get BE
	var tmp := PackedByteArray()
	tmp.resize(4)
	tmp.encode_float(0, val)
	var be := PackedByteArray()
	be.append(tmp[3]); be.append(tmp[2]); be.append(tmp[1]); be.append(tmp[0])
	return be

func _unpack_f32_be(buf: PackedByteArray, offset: int) -> float:
	var tmp := PackedByteArray()
	tmp.append(buf[offset + 3]); tmp.append(buf[offset + 2])
	tmp.append(buf[offset + 1]); tmp.append(buf[offset + 0])
	return tmp.decode_float(0)

func _i8_to_u8(val: int) -> int:
	# clamp to -1/0/1 then cast to u8 (matches Rust's i8 as u8)
	val = clampi(val, -1, 1)
	if val < 0:
		return val + 256
	return val

func export_drift_csv() -> void:
	var file = FileAccess.open("user://drift_log.csv", FileAccess.WRITE)
	if file == null:
		push_error("Failed to open drift log file")
		return
	file.store_line("time_seconds,drift_units")
	for entry in _drift_log:
		file.store_line("%.3f,%.4f" % [entry["time"], entry["drift"]])
	file.close()
	print("Drift log saved to: ", OS.get_user_data_dir(), "/drift_log.csv")
