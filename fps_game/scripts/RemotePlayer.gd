extends CharacterBody3D
# RemotePlayer.gd
# Represents another player in the world.
# Has NO input logic - position and state are set entirely
# by whatever NetworkManager tells it.

@onready var name_label: Label3D = $NameLabel
@onready var mesh: MeshInstance3D = $PlayerMesh
@onready var health_bar: Node3D = $HealthBar

var player_id: int = -1
var current_health: int = 100

# Interpolation - smooth movement between received state updates
var _target_position: Vector3 = Vector3.ZERO
var _target_yaw: float = 0.0
var _interpolation_speed: float = 12.0

func _ready() -> void:
	add_to_group("remote_players")

func setup(pid: int) -> void:
	player_id = pid
	name_label.text = "Player%d" % pid

func apply_state(pos: Vector3, yaw: float) -> void:
	# Called by NetworkManager when a state update arrives for this player
	_target_position = pos
	_target_yaw = yaw

func _physics_process(delta: float) -> void:
	# Interpolate toward target smoothly to hide packet jitter
	global_position = global_position.lerp(_target_position, _interpolation_speed * delta)
	rotation.y = lerp_angle(rotation.y, _target_yaw, _interpolation_speed * delta)

func _update_health_bar() -> void:
	if health_bar:
		var bar = health_bar.get_node_or_null("Bar")
		if bar:
			bar.scale.x = float(current_health) / 100.0
