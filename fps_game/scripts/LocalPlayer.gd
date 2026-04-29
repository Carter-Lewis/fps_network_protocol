extends CharacterBody3D

const SPEED = 6.0
const JUMP_VELOCITY = 5.0
const MOUSE_SENSITIVITY = 0.002
const GRAVITY = 9.8

@onready var camera: Camera3D = $CameraMount/Camera3D
@onready var camera_mount: Node3D = $CameraMount
@onready var hud: CanvasLayer = $HUD
@onready var health_label: Label = $HUD/HealthLabel
@onready var crosshair: TextureRect = $HUD/Crosshair
@onready var hit_marker: TextureRect = $HUD/HitMarker

var health: int = 100
var _hit_marker_timer: float = 0.0
var _swing_cooldown: float = 0.0
var _stick: MeshInstance3D = null
var _stick_rest_rotation: Vector3 = Vector3(-0.3, 0.2, 0.0)

func _ready() -> void:
	Input.set_mouse_mode(Input.MOUSE_MODE_CAPTURED)
	NetworkManager.register_local_player(self)
	_spawn_stick()

func _spawn_stick() -> void:
	_stick = MeshInstance3D.new()
	var mesh = BoxMesh.new()
	mesh.size = Vector3(0.05, 0.05, 0.6)
	_stick.mesh = mesh
	_stick.position = Vector3(0.3, -0.3, -0.4)
	_stick.rotation = _stick_rest_rotation
	camera_mount.add_child(_stick)

func _unhandled_input(event: InputEvent) -> void:
	if event is InputEventMouseMotion:
		rotate_y(-event.relative.x * MOUSE_SENSITIVITY)
		camera_mount.rotate_x(-event.relative.y * MOUSE_SENSITIVITY)
		camera_mount.rotation.x = clamp(camera_mount.rotation.x, -PI/2.2, PI/2.2)
	if event.is_action_pressed("shoot") and _swing_cooldown <= 0.0:
		_do_swing()
	if event is InputEventKey and event.pressed:
		if event.keycode == KEY_ESCAPE:
			Input.set_mouse_mode(Input.MOUSE_MODE_VISIBLE)
		if event.keycode == KEY_F1:
			Input.set_mouse_mode(Input.MOUSE_MODE_CAPTURED)

func _physics_process(delta: float) -> void:
	_swing_cooldown -= delta
	_hit_marker_timer -= delta
	if _hit_marker_timer <= 0:
		hit_marker.visible = false
	if not is_on_floor():
		velocity.y -= GRAVITY * delta
	var input_dir = Input.get_vector("move_left", "move_right", "move_forward", "move_backward")
	var direction = (transform.basis * Vector3(input_dir.x, 0, input_dir.y)).normalized()
	if is_on_floor():
		velocity.x = direction.x * SPEED
		velocity.z = direction.z * SPEED
	else:
		velocity.x = lerp(velocity.x, direction.x * SPEED, delta * 3.0)
		velocity.z = lerp(velocity.z, direction.z * SPEED, delta * 3.0)
	if Input.is_action_just_pressed("jump") and is_on_floor():
		velocity.y = JUMP_VELOCITY
	move_and_slide()

func _do_swing() -> void:
	_swing_cooldown = 0.5
	NetworkManager.send_swing()
	hit_marker.visible = true
	_hit_marker_timer = 0.2
	# animate stick forward and back
	var tween = create_tween()
	tween.tween_property(_stick, "rotation", Vector3(-1.0, 0.2, 0.0), 0.1)
	tween.tween_property(_stick, "rotation", _stick_rest_rotation, 0.15)
