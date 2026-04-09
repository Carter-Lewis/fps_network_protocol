extends CharacterBody3D

# RemotePlayer.gd
# Represents another player in the world.
# Has NO input logic - position and state are set entirely
# by whatever NetworkManager tells it.
# This is where client-side interpolation will live.

@onready var name_label: Label3D = $NameLabel
@onready var mesh: MeshInstance3D = $PlayerMesh
@onready var health_bar: Node3D = $HealthBar

var player_id: int = -1
var player_name: String = "?"
var player_color: Color = Color.WHITE
var current_health: int = 100

# Interpolation - smooth movement between received state updates
var _target_position: Vector3 = Vector3.ZERO
var _target_rotation: Vector2 = Vector2.ZERO
var _interpolation_speed: float = 12.0

# Drift tracking - for your evaluation component
var _last_authoritative_position: Vector3 = Vector3.ZERO
var _position_before_interpolation: Vector3 = Vector3.ZERO

func _ready() -> void:
	add_to_group("remote_players")

func setup(pid: int, data: Dictionary) -> void:
	player_id = pid
	player_name = data.get("name", "Player%d" % pid)
	player_color = data.get("color", Color.WHITE)

	name_label.text = player_name

	# Tint the player mesh to their color
	var mat = StandardMaterial3D.new()
	mat.albedo_color = player_color
	mesh.set_surface_override_material(0, mat)

	var pos = data.get("position", Vector3.ZERO)
	global_position = pos
	_target_position = pos

func apply_state(state: Dictionary) -> void:
	# Called by NetworkManager when a state update arrives for this player
	_position_before_interpolation = global_position
	_last_authoritative_position = state.get("position", _target_position)
	_target_position = _last_authoritative_position
	_target_rotation = state.get("rotation", _target_rotation)

	var new_health = state.get("health", current_health)
	if new_health != current_health:
		current_health = new_health
		_update_health_bar()

	# Log drift for evaluation
	var drift = _position_before_interpolation.distance_to(_last_authoritative_position)
	if drift > 0.01:
		NetworkManager.log_drift(player_id, _last_authoritative_position, _position_before_interpolation)

func _physics_process(delta: float) -> void:
	# Interpolate toward the target position smoothly
	# This hides packet-to-packet jitter visually
	global_position = global_position.lerp(_target_position, _interpolation_speed * delta)
	rotation.y = lerp_angle(rotation.y, _target_rotation.x, _interpolation_speed * delta)

func _update_health_bar() -> void:
	if health_bar:
		var bar = health_bar.get_node_or_null("Bar")
		if bar:
			bar.scale.x = float(current_health) / 100.0

func take_hit(damage: int) -> void:
	current_health = max(0, current_health - damage)
	_update_health_bar()
	if current_health <= 0:
		_on_died()

func _on_died() -> void:
	# Flash red, then respawn after delay
	var mat = StandardMaterial3D.new()
	mat.albedo_color = Color.RED
	mesh.set_surface_override_material(0, mat)
	await get_tree().create_timer(1.5).timeout
	current_health = 100
	_update_health_bar()
	var mat2 = StandardMaterial3D.new()
	mat2.albedo_color = player_color
	mesh.set_surface_override_material(0, mat2)
