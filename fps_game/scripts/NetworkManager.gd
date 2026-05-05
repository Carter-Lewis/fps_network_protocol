extends Node

@export var use_cloud := true

const CLOUD_IP := "server.bayloruif.com"
const LOCAL_IP := "127.0.0.1"
const TOKYO_IP := "57.181.105.56"
# Paste the SHA-256 base64 fingerprint printed by the server on startup.
# Leave empty ("") when using a CA-signed cert (Let's Encrypt, etc.).
const CERT_HASH_B64 := ""  # empty = CA-signed cert (Let's Encrypt); set to fingerprint for self-signed

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

# transport state
var use_webtransport: bool = OS.get_name() == "Web"

# Variables for tracking if server stops communicating
var _last_packet_time: float = 0.0
const TIMEOUT_SECONDS := 5.0

# sockets
var tcp: StreamPeerTCP
var udp: PacketPeerUDP
var my_udp_port: int = 0
var _webtransport_ready := false

# scene refs
var local_player: Node = null
var remote_players: Dictionary = {}  # player_id (int) -> RemotePlayer node

var _drift_log: Array = []
var _show_drift_ui := false
var _start_time: float = 0.0
var _last_world_state_seq: int = -1  # tracks monotonically increasing world state ticks

signal player_joined(player_id: int)

func _ready():
	_start_time = Time.get_ticks_msec() / 1000.0
	print("[NetworkManager] Platform: %s, Using WebTransport: %s" % [OS.get_name(), use_webtransport])
	# wrap in CanvasLayer so it renders over the game
	var canvas = CanvasLayer.new()
	canvas.name = "DriftCanvas"
	add_child(canvas)
	var label = Label.new()
	label.name = "DriftLabel"
	label.anchor_left = 1.0
	label.anchor_right = 1.0
	label.anchor_top = 0.0
	label.anchor_bottom = 0.0
	label.offset_left = -200.0
	label.offset_right = -10.0
	label.offset_top = 10.0
	label.horizontal_alignment = HORIZONTAL_ALIGNMENT_RIGHT
	label.visible = false
	canvas.add_child(label)

func _send_webtransport_stream(buf: PackedByteArray) -> void:
	JavaScriptBridge.eval("webtransportBridge.sendStream(%s);" % _packed_byte_array_to_js_array(buf), true)
	  
func connect_to_server() -> void:
	if use_webtransport:
		await _connect_webtransport()
	else:
		_start_udp()
		await _connect_tcp_with_retry()

func _webtransport_url() -> String:
	match active_server:
		Server.LOCAL: return "https://localhost:7777/wt"
		Server.CLOUD: return "https://server.bayloruif.com:7777/wt"
		Server.TOKYO: return "https://tokyo.notinuse.com/wt"
		_: return "https://localhost:7777/wt"

func _send_webtransport_packet(buf: PackedByteArray) -> void:
	if not _webtransport_ready:
		return
	JavaScriptBridge.eval("webtransportBridge.sendDatagram(%s);" % _packed_byte_array_to_js_array(buf), true)

func _csv_to_bytes(csv: String) -> PackedByteArray:
	var bytes := PackedByteArray()
	for s in csv.split(","):
		bytes.append(int(s))
	return bytes

func _read_webtransport_packet() -> PackedByteArray:
	var csv = JavaScriptBridge.eval("webtransportBridge.receiveDatagram();", true)
	if csv == null or typeof(csv) != TYPE_STRING or csv == "":
		return PackedByteArray()
	return _csv_to_bytes(csv)

func _poll_webtransport_streams() -> void:
	while true:
		var csv = JavaScriptBridge.eval("webtransportBridge.receiveStream();", true)
		if csv == null or typeof(csv) != TYPE_STRING or csv == "":
			break
		var bytes := _csv_to_bytes(csv)
		if bytes.is_empty():
			break
		match bytes[0]:
			0x10:
				if bytes.size() >= 3:
					my_player_id = _unpack_u16(bytes, 1)
					_last_packet_time = Time.get_ticks_msec() / 1000.0
					print("Connected! player_id = ", my_player_id)
			0x13:
				if bytes.size() >= 7:
					var _pid = _unpack_u16(bytes, 1)
					var h = _unpack_i32_be(bytes, 3)
					if local_player and local_player.has_method("update_health"):
						local_player.update_health(h)
			0x14:
				if bytes.size() >= 3:
					var _pid = _unpack_u16(bytes, 1)
					if local_player and local_player.has_method("on_death"):
						local_player.on_death()

func _packed_byte_array_to_js_array(buf: PackedByteArray) -> String:
	var parts: Array[String] = []
	for byte in buf:
		parts.append(str(byte))
	return "[" + ",".join(parts) + "]"

func _connect_webtransport() -> void:
	var url := _webtransport_url()
	print("[WebTransport] Connecting to: ", url)
	JavaScriptBridge.eval("webtransportBridge.connectAsync('%s', '%s');" % [url, CERT_HASH_B64], true)
	await _wait_webtransport_connected()
	_send_connect_webtransport()

func _wait_webtransport_connected() -> void:
	var timeout := 10.0
	var elapsed := 0.0
	while elapsed < timeout:
		var connected: Variant = JavaScriptBridge.eval("webtransportBridge.isConnectedStatus();", true)
		var error: Variant = JavaScriptBridge.eval("webtransportBridge.getConnectionError();", true)
		
		if error:
			push_error("[WebTransport] Connection error: %s" % error)
			return
		
		if bool(connected):
			_webtransport_ready = true
			print("[WebTransport] Connected successfully")
			return
		
		await get_tree().process_frame
		elapsed += get_process_delta_time()
	
	push_error("[WebTransport] Connection timed out after %.1f seconds" % timeout)

func _send_connect_webtransport() -> void:
	var buf := PackedByteArray()
	buf.append(0x01)
	buf.append(0x00)
	buf.append(0x00)
	_send_webtransport_stream(buf)
	print("Sent WebTransport CONNECT")

# bind udp first so we know our local port before tcp handshake
func _start_udp():
	udp = PacketPeerUDP.new()
	udp.bind(0)
	my_udp_port = udp.get_local_port()
	udp.set_dest_address(server_ip, udp_port_server)
	print("UDP bound on local port: ", my_udp_port)
	
func _connect_tcp_with_retry() -> void:
	var attempts := 0
	var max_attempts := 4
	while attempts < max_attempts:
		attempts += 1
		print("Connection attempt %d of %d..." % [attempts, max_attempts])
		_connect_tcp()
		await _wait_tcp_connected()
		if tcp != null and tcp.get_status() == StreamPeerTCP.STATUS_CONNECTED:
			_send_connect()
			return
		print("Connection failed, retrying in 2 seconds...")
		tcp = null
		await get_tree().create_timer(2.0).timeout

	print("Could not connect after %d attempts, returning to menu" % max_attempts)
	my_player_id = -1
	_last_packet_time = 0.0
	local_player = null
	remote_players.clear()
	Input.mouse_mode = Input.MOUSE_MODE_VISIBLE
	get_tree().change_scene_to_file("res://scenes/MainMenu.tscn")

# tcp connect then send connect packet
func _connect_tcp():
	print("Connecting to: ", server_ip, ":", tcp_port)
	tcp = StreamPeerTCP.new()
	tcp.set_no_delay(true)
	tcp.connect_to_host(server_ip, tcp_port)

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
	if use_webtransport:
		_poll_webtransport_streams()
	_poll_tcp()
	if my_player_id >= 0:
		_send_player_input()
		_poll_udp()
		_check_timeout()
	# update drift label if visible
	if _show_drift_ui:
		_update_drift_label()
		
func _check_timeout() -> void:
	if _last_packet_time == 0.0:
		return
	var now = Time.get_ticks_msec() / 1000.0
	if now - _last_packet_time > TIMEOUT_SECONDS:
		print("Server timeout, returning to menu")
		# reset state so we don't trigger again
		my_player_id = -1
		_last_packet_time = 0.0
		local_player = null
		remote_players.clear()
		get_tree().change_scene_to_file("res://scenes/MainMenu.tscn")

# f2 = export csv, f3 = toggle drift ui
func _input(event: InputEvent) -> void:
	if event is InputEventKey and event.pressed and not event.echo:
		if event.keycode == KEY_F2:
			export_drift_csv()
		elif event.keycode == KEY_F3:
			_show_drift_ui = !_show_drift_ui
			get_node("DriftCanvas/DriftLabel").visible = _show_drift_ui

# tcp in: read connected (0x10) and store assigned player id
func _poll_tcp():
	if tcp == null:
		return
	tcp.poll()
	if tcp.get_status() != StreamPeerTCP.STATUS_CONNECTED:
		return
	while tcp.get_available_bytes() > 0:
		var type_byte = tcp.get_u8()
		if type_byte == 0x10:
			# connected - 3 bytes total, already read 1
			if tcp.get_available_bytes() < 2:
				return
			var hi = tcp.get_u8()
			var lo = tcp.get_u8()
			my_player_id = (hi << 8) | lo
			print("Connected! player_id = ", my_player_id)
		elif type_byte == 0x13:
			# health update - 6 more bytes
			if tcp.get_available_bytes() < 6:
				return
			var hi = tcp.get_u8()
			var lo = tcp.get_u8()
			var _pid = (hi << 8) | lo
			var h = tcp.get_u8() << 24 | tcp.get_u8() << 16 | tcp.get_u8() << 8 | tcp.get_u8()
			if local_player and local_player.has_method("update_health"):
				local_player.update_health(h)
		elif type_byte == 0x14:
			# you died - 2 more bytes
			if tcp.get_available_bytes() < 2:
				return
			var hi = tcp.get_u8()
			var lo = tcp.get_u8()
			var _pid = (hi << 8) | lo
			if local_player and local_player.has_method("on_death"):
				local_player.on_death()

# udp out: send player input (0x02) every frame
# layout: [type:u8, seq:u16be, yaw:f32be, pitch:f32be, move_x:i8, move_z:i8]
func _send_player_input():
	if use_webtransport:
		_send_player_input_webtransport()
		return
	if udp == null or local_player == null:
		return
	var move_x: int = 0
	var move_z: int = 0
	if Input.is_action_pressed("move_right"):    move_x += 1
	if Input.is_action_pressed("move_left"):     move_x -= 1
	if Input.is_action_pressed("move_backward"): move_z += 1
	if Input.is_action_pressed("move_forward"):  move_z -= 1
	
	var flags: int = 0
	if Input.is_action_just_pressed("jump"):
		flags |= 0x01
	
	var yaw: float = local_player.rotation.y
	var pitch: float = 0.0
	if local_player.has_node("CameraMount"):
		pitch = local_player.get_node("CameraMount").rotation.x
	
	var local_y: float = local_player.global_position.y - 1.0
	
	input_seq = (input_seq + 1) % 65536
	var buf := PackedByteArray()
	buf.append(0x02)
	buf.append_array(_pack_u16(my_player_id))
	buf.append_array(_pack_u16(input_seq))
	buf.append_array(_pack_f32_be(yaw))
	buf.append_array(_pack_f32_be(pitch))
	buf.append(_i8_to_u8(move_x))
	buf.append(_i8_to_u8(move_z))
	buf.append_array(_pack_f32_be(local_y))
	buf.append(flags)
	udp.put_packet(buf)

func _send_player_input_webtransport() -> void:
	if local_player == null or not _webtransport_ready:
		return
	var move_x: int = 0
	var move_z: int = 0
	if Input.is_action_pressed("move_right"): move_x += 1
	if Input.is_action_pressed("move_left"): move_x -= 1
	if Input.is_action_pressed("move_backward"): move_z += 1
	if Input.is_action_pressed("move_forward"): move_z -= 1

	var flags: int = 0
	if Input.is_action_just_pressed("jump"):
		flags |= 0x01

	var yaw: float = local_player.rotation.y
	var pitch: float = 0.0
	if local_player.has_node("CameraMount"):
		pitch = local_player.get_node("CameraMount").rotation.x

	var local_y: float = local_player.global_position.y - 1.0

	input_seq = (input_seq + 1) % 65536
	var buf := PackedByteArray()
	buf.append(0x02)
	buf.append_array(_pack_u16(my_player_id))
	buf.append_array(_pack_u16(input_seq))
	buf.append_array(_pack_f32_be(yaw))
	buf.append_array(_pack_f32_be(pitch))
	buf.append(_i8_to_u8(move_x))
	buf.append(_i8_to_u8(move_z))
	buf.append_array(_pack_f32_be(local_y))
	buf.append(flags)
	_send_webtransport_packet(buf)

func send_respawn_request() -> void:
	if my_player_id < 0:
		return
	var buf := PackedByteArray()
	buf.append(0x15)
	buf.append((my_player_id >> 8) & 0xFF)
	buf.append(my_player_id & 0xFF)
	if use_webtransport:
		_send_webtransport_stream(buf)
	else:
		tcp.put_data(buf)
	print("Sent RespawnRequest for player ", my_player_id)

# udp in: route incoming packets by type
func _poll_udp():
	if use_webtransport:
		while true:
			var packet := _read_webtransport_packet()
			if packet.is_empty():
				break
			_last_packet_time = Time.get_ticks_msec() / 1000.0
			match packet[0]:
				0x11:
					_handle_world_state(packet)
				0x12:
					_handle_player_left(packet)
				0x04:
					_handle_swing_notify(packet)
		return
	if udp == null:
		return
	while udp.get_available_packet_count() > 0:
		_last_packet_time = Time.get_ticks_msec() / 1000.0
		var packet = udp.get_packet()
		if packet.size() < 1:
			continue
		match packet[0]:
			0x11:
				_handle_world_state(packet)
			0x12:
				_handle_player_left(packet)
			0x04:
				_handle_swing_notify(packet)

# tcp out: send swing packet (0x03) with our player id

func send_swing() -> void: 
	if my_player_id < 0:
		return
	var buf := PackedByteArray()
	buf.append(0x03)
	buf.append((my_player_id >> 8) & 0xFF)
	buf.append(my_player_id & 0xFF)
	if use_webtransport:
		_send_webtransport_stream(buf)
	else:
		tcp.put_data(buf)
	print("Sent Swing for player ", my_player_id)
	
# udp in: another player swung, play their animation
func _handle_swing_notify(packet: PackedByteArray):
	if packet.size() < 3:
		return
	var pid = _unpack_u16(packet, 1)
	if remote_players.has(pid):
		var rp = remote_players[pid]
		if is_instance_valid(rp) and rp.has_method("play_swing"):
			rp.play_swing()

# udp in: parse world state and apply positions to remote players
# layout: [type:u8, count:u8, then count x 26 bytes per player]
func _handle_world_state(packet: PackedByteArray):
	var player_count = packet[1]
	var expected = 2 + player_count * 26
	if packet.size() != expected:
		push_warning("WorldState size mismatch: got %d, expected %d" % [packet.size(), expected])
		return
	# QUIC datagrams can arrive out of order; use a monotonic tick counter derived
	# from the server's broadcast cadence to drop stale packets before cleanup.
	var now_msec := Time.get_ticks_msec()
	var is_latest := now_msec >= _last_world_state_seq
	if is_latest:
		_last_world_state_seq = now_msec
	var offset = 2
	var seen_pids: Array = []
	for i in range(player_count):
		var pid    = _unpack_u16(packet, offset);    offset += 2
		var px     = _unpack_f32_be(packet, offset); offset += 4
		var py     = _unpack_f32_be(packet, offset); offset += 4
		var pz     = _unpack_f32_be(packet, offset); offset += 4
		var yaw    = _unpack_f32_be(packet, offset); offset += 4
		var pitch  = _unpack_f32_be(packet, offset); offset += 4
		var health = _unpack_i32_be(packet, offset); offset += 4
		seen_pids.append(pid)
		if pid == my_player_id:
			_reconcile_local(Vector3(px, py, pz))
		else:
			_apply_remote(pid, Vector3(px, py, pz), yaw, health)

	# Only run cleanup when this is the most-recent packet seen so far;
	# skipping cleanup on out-of-order datagrams prevents spurious despawns.
	if not is_latest:
		return
	for pid in remote_players.keys():
		if pid not in seen_pids:
			var node = remote_players[pid]
			if is_instance_valid(node):
				node.queue_free()
			remote_players.erase(pid)
			print("[-] Player ", pid, " removed from world state, despawned")

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
	_drift_log.append({"time": elapsed, "player_id": my_player_id, "drift": drift})
	if drift > 0.5:
		var corrected = Vector3(server_pos.x, local_player.global_position.y, server_pos.z)
		local_player.global_position = local_player.global_position.lerp(corrected, 0.3)

# apply server position and yaw to a remote player node, log drift
func _apply_remote(pid: int, pos: Vector3, yaw: float, health: int):
	if not remote_players.has(pid):
		emit_signal("player_joined", pid)
		# signal is synchronous: if spawning succeeded, remote_players[pid] is now set
		if not remote_players.has(pid):
			return  # our own player_id or spawn failed
	var rp = remote_players[pid]
	if is_instance_valid(rp):
		var drift = pos.distance_to(rp.global_position)
		var elapsed = (Time.get_ticks_msec() / 1000.0) - _start_time
		_drift_log.append({"time": elapsed, "player_id": pid, "drift": drift})
		rp.apply_state(pos, yaw, health)

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
	
func _unpack_i32_be(buf: PackedByteArray, offset: int) -> int:
	var val = (buf[offset] << 24) | (buf[offset+1] << 16) | (buf[offset+2] << 8) | buf[offset+3]
	# sign extend
	if val >= 0x80000000:
		val -= 0x100000000
	return val

# export drift log to csv, one row per player per sample
func export_drift_csv() -> void:
	var file = FileAccess.open("user://drift_log.csv", FileAccess.WRITE)
	if file == null:
		push_error("Failed to open drift log file")
		return
	file.store_line("time_seconds,player_id,drift_units")
	for entry in _drift_log:
		file.store_line("%.3f,%d,%.4f" % [entry["time"], entry["player_id"], entry["drift"]])
	file.close()
	print("Drift log saved to: ", OS.get_user_data_dir(), "/drift_log.csv")

# build and update the on-screen drift label from recent log entries
func _update_drift_label():
	var label = get_node("DriftCanvas/DriftLabel")
	if _drift_log.is_empty():
		label.text = "no drift data yet"
		return
	# collect most recent drift per player
	var latest: Dictionary = {}
	for entry in _drift_log:
		latest[entry["player_id"]] = entry["drift"]
	var lines = ["-- drift (F3 to hide) --"]
	for pid in latest:
		var tag = "me" if pid == my_player_id else "p%d" % pid
		lines.append("%s: %.4f" % [tag, latest[pid]])
	label.text = "\n".join(lines)
