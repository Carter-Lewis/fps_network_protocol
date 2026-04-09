extends Node

# NetworkManager.gd
# ============================================================
# THIS IS THE STUB YOUR RUST PROTOCOL WILL REPLACE.
# All multiplayer state flows through this singleton.
# Godot itself does zero networking - this manager is the
# single integration point between the game and your UDP protocol.
# ============================================================

signal player_joined(player_id: int, player_data: Dictionary)
signal player_left(player_id: int)
signal player_state_updated(player_id: int, state: Dictionary)
signal hit_confirmed(shooter_id: int, target_id: int, damage: int)

# Our local player ID (assigned by server in real implementation)
var local_player_id: int = 1

# All known players: { player_id: { position, rotation, health, name, color } }
var player_states: Dictionary = {}

# Sequence number for outgoing messages
var _seq: int = 0

# Simulated network stats (for testing without Rust server)
var simulate_local: bool = true

func _ready() -> void:
	if simulate_local:
		_spawn_dummy_players()

# ============================================================
# OUTGOING - called by game, send to Rust server
# ============================================================

func send_player_input(move_dir: Vector3, look_rotation: Vector2, jumped: bool) -> void:
	_seq += 1
	# TODO: Serialize into your binary protocol and send over UDP
	# Message type: 0x01 - PlayerInput
	# Fields: seq, player_id, move_dir (x,y,z), look_rotation (yaw,pitch), jumped
	var msg = {
		"type": 0x01,
		"seq": _seq,
		"player_id": local_player_id,
		"move_dir": move_dir,
		"look_rotation": look_rotation,
		"jumped": jumped,
		"timestamp": Time.get_ticks_msec()
	}
	_stub_loopback(msg)

func send_shoot_event(origin: Vector3, direction: Vector3) -> void:
	_seq += 1
	# TODO: Serialize and send over UDP - RELIABLE channel (must arrive)
	# Message type: 0x02 - ShootEvent
	var msg = {
		"type": 0x02,
		"seq": _seq,
		"player_id": local_player_id,
		"origin": origin,
		"direction": direction,
		"timestamp": Time.get_ticks_msec()
	}
	_stub_loopback(msg)

# ============================================================
# INCOMING - called by Rust protocol when packet arrives
# ============================================================

func receive_world_state(state_data: Dictionary) -> void:
	# Called every server tick with authoritative state
	# state_data = { players: { id: { pos, rot, health } }, tick: int }
	for pid in state_data.get("players", {}):
		var pstate = state_data["players"][pid]
		apply_player_state(int(pid), pstate)

func receive_hit_confirmed(shooter_id: int, target_id: int, damage: int) -> void:
	# Reliable message - hit was confirmed by server
	if player_states.has(target_id):
		player_states[target_id]["health"] -= damage
		player_states[target_id]["health"] = max(0, player_states[target_id]["health"])
	emit_signal("hit_confirmed", shooter_id, target_id, damage)

func apply_player_state(player_id: int, state: Dictionary) -> void:
	if not player_states.has(player_id):
		player_states[player_id] = _default_player_state(player_id)
		emit_signal("player_joined", player_id, player_states[player_id])
	player_states[player_id].merge(state, true)
	emit_signal("player_state_updated", player_id, player_states[player_id])

# ============================================================
# DRIFT MEASUREMENT - for your evaluation component
# ============================================================

var drift_log: Array = []

func log_drift(player_id: int, expected_pos: Vector3, actual_pos: Vector3) -> void:
	var drift = expected_pos.distance_to(actual_pos)
	drift_log.append({
		"tick": Time.get_ticks_msec(),
		"player_id": player_id,
		"drift": drift,
		"expected": expected_pos,
		"actual": actual_pos
	})

func export_drift_log() -> String:
	# Call this at end of session to get CSV for your graphs
	var csv = "tick,player_id,drift\n"
	for entry in drift_log:
		csv += "%d,%d,%.4f\n" % [entry["tick"], entry["player_id"], entry["drift"]]
	return csv

# ============================================================
# LOCAL SIMULATION STUB (no Rust server needed yet)
# ============================================================

func _spawn_dummy_players() -> void:
	# Spawn 5 dummy remote players at startup for testing
	var colors = [
		Color.RED, Color.BLUE, Color.GREEN, Color.YELLOW, Color.PURPLE
	]
	var names = ["Alpha", "Bravo", "Charlie", "Delta", "Echo"]
	for i in range(5):
		var pid = i + 2  # player IDs 2-6, local player is 1
		player_states[pid] = {
			"position": Vector3(randf_range(-10, 10), 1.0, randf_range(-10, 10)),
			"rotation": Vector2(randf_range(-PI, PI), 0.0),
			"health": 100,
			"name": names[i],
			"color": colors[i],
			"player_id": pid
		}
		emit_signal("player_joined", pid, player_states[pid])

func _stub_loopback(msg: Dictionary) -> void:
	# Simulates server echo for local testing
	# Remove when Rust server is connected
	pass

func _default_player_state(player_id: int) -> Dictionary:
	return {
		"position": Vector3.ZERO,
		"rotation": Vector2.ZERO,
		"health": 100,
		"name": "Player%d" % player_id,
		"color": Color.WHITE,
		"player_id": player_id
	}
