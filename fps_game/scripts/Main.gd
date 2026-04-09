extends Node3D

# Main.gd
# Manages the game world. Spawns and despawns players
# based on signals from NetworkManager.

@onready var remote_players_container: Node3D = $RemotePlayers
@onready var drift_ui: Label = $DriftUI/Label

var remote_player_scene: PackedScene = preload("res://scenes/RemotePlayer.tscn")
var _remote_player_nodes: Dictionary = {}

# Drift display update timer
var _drift_update_timer: float = 0.0

func _ready() -> void:
	NetworkManager.connect("player_joined", _on_player_joined)
	NetworkManager.connect("player_left", _on_player_left)
	NetworkManager.connect("player_state_updated", _on_player_state_updated)

func _process(delta: float) -> void:
	_drift_update_timer -= delta
	if _drift_update_timer <= 0.0:
		_drift_update_timer = 0.5
		_update_drift_display()

func _on_player_joined(player_id: int, player_data: Dictionary) -> void:
	if player_id == NetworkManager.local_player_id:
		return  # Don't spawn a remote node for ourselves
	if _remote_player_nodes.has(player_id):
		return  # Already spawned

	var player = remote_player_scene.instantiate()
	remote_players_container.add_child(player)
	player.setup(player_id, player_data)
	_remote_player_nodes[player_id] = player
	print("Player joined: ", player_id)

func _on_player_left(player_id: int) -> void:
	if _remote_player_nodes.has(player_id):
		_remote_player_nodes[player_id].queue_free()
		_remote_player_nodes.erase(player_id)
	print("Player left: ", player_id)

func _on_player_state_updated(player_id: int, state: Dictionary) -> void:
	if player_id == NetworkManager.local_player_id:
		return
	if _remote_player_nodes.has(player_id):
		_remote_player_nodes[player_id].apply_state(state)

func _update_drift_display() -> void:
	# Show recent drift stats on screen for demo/debug
	var log = NetworkManager.drift_log
	if log.is_empty():
		drift_ui.text = "Drift: measuring..."
		return

	var recent = log.slice(max(0, log.size() - 20), log.size())
	var total = 0.0
	var max_drift = 0.0
	for entry in recent:
		total += entry["drift"]
		max_drift = max(max_drift, entry["drift"])

	var avg = total / recent.size()
	drift_ui.text = "Avg drift: %.3fm  Max: %.3fm" % [avg, max_drift]

func _input(event: InputEvent) -> void:
	if event is InputEventKey and event.pressed:
		# F2: export drift log to file
		if event.keycode == KEY_F2:
			_save_drift_log()

func _save_drift_log() -> void:
	var csv = NetworkManager.export_drift_log()
	var file = FileAccess.open("user://drift_log.csv", FileAccess.WRITE)
	if file:
		file.store_string(csv)
		file.close()
		print("Drift log saved to drift_log.csv")
