extends CharacterBody3D
# RemotePlayer.gd
# Represents another player in the world.
# Has NO input logic - position and state are set entirely
# by whatever NetworkManager tells it.

@onready var name_label: Label3D = $NameLabel
@onready var mesh: MeshInstance3D = $MeshInstance3D
@onready var health_bar: Node3D = $HealthBar


var player_id: int = -1
var current_health: int = 100
var _stick: MeshInstance3D = null
var _stick_rest_rotation: Vector3 = Vector3(deg_to_rad(-14.4), deg_to_rad(-73.4), deg_to_rad(-34.2))


# Interpolation - smooth movement between received state updates
var _target_position: Vector3 = Vector3.ZERO
var _target_yaw: float = 0.0
var _interpolation_speed: float = 12.0

func _ready() -> void:
	add_to_group("remote_players")
	_spawn_stick()
	print("RemotePlayer ready, stick: ", _stick)

func setup(pid: int) -> void:
	player_id = pid
	name_label.text = "Player%d" % pid

func apply_state(pos: Vector3, yaw: float, health: int) -> void:
	# Called by NetworkManager when a state update arrives for this player
	_target_position = pos
	_target_yaw = yaw
	current_health = health
	_update_health_bar()

func _physics_process(delta: float) -> void:
	# Interpolate toward target smoothly to hide packet jitter
	global_position = global_position.lerp(_target_position, _interpolation_speed * delta)
	rotation.y = lerp_angle(rotation.y, _target_yaw, _interpolation_speed * delta)

func _update_health_bar() -> void:
	var bar = health_bar.get_node_or_null("Bar")
	if bar:
		var pct = float(current_health) / 100.0
		bar.scale.x = pct
		bar.position.x = (1.0 - pct) * 0.5

func _spawn_stick() -> void:
	var stick_mesh = load("res://assets/resource-wood.obj")
	_stick = MeshInstance3D.new()
	_stick.mesh = stick_mesh
	_stick.position = Vector3(0.485, 1.559, -0.76)
	_stick.rotation = _stick_rest_rotation
	_stick.scale = Vector3(5.0, 5.0, 5.0)
	var mat = StandardMaterial3D.new()
	mat.albedo_color = Color(0.55, 0.35, 0.15)
	mat.shading_mode = BaseMaterial3D.SHADING_MODE_PER_PIXEL
	_stick.material_override = mat
	add_child(_stick)
	
func play_swing() -> void:
	var tween = create_tween()
	tween.tween_property(_stick, "rotation", Vector3(deg_to_rad(-14.4), deg_to_rad(-73.4), 1.2), 0.15)
	tween.tween_property(_stick, "rotation", _stick_rest_rotation, 0.2)
