extends Node3D
# Main.gd
# Manages the game world. Spawns and despawns players
# based on signals from NetworkManager.

@onready var remote_players_container: Node3D = $RemotePlayers
@onready var drift_ui: Label = $DriftUI/Label

var remote_player_scene: PackedScene = preload("res://scenes/RemotePlayer.tscn")
var _remote_player_nodes: Dictionary = {}
var _drift_update_timer: float = 0.0

func _ready() -> void:
	NetworkManager.connect("player_joined", _on_player_joined)

func _process(delta: float) -> void:
	_drift_update_timer -= delta
	# changed to help with lag
	global_position = global_position.lerp(target_position, delta * 12.0)
	if _drift_update_timer <= 0.0:
		_drift_update_timer = 0.5
		_update_drift_display()

func _on_player_joined(player_id: int) -> void:
	if player_id == NetworkManager.my_player_id:
		return
	# re-spawn if node was previously despawned (e.g. after death)
	if _remote_player_nodes.has(player_id):
		var existing = _remote_player_nodes[player_id]
		if is_instance_valid(existing):
			return  # already alive, skip
		else:
			_remote_player_nodes.erase(player_id)  # stale, clean up and re-spawn
	var player = remote_player_scene.instantiate()
	remote_players_container.add_child(player)
	player.setup(player_id)
	_remote_player_nodes[player_id] = player
	NetworkManager.register_remote_player(player_id, player)
	print("Player joined: ", player_id)

func _update_drift_display() -> void:
	# Placeholder until Person 1 server is sending real data
	if NetworkManager.my_player_id < 0:
		drift_ui.text = "Connecting..."
	else:
		drift_ui.text = "Connected | player_id = %d" % NetworkManager.my_player_id
