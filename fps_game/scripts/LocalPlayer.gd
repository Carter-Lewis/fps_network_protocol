extends CharacterBody3D

const SPEED = 6.0
const JUMP_VELOCITY = 5.0
const MOUSE_SENSITIVITY = 0.002
const GRAVITY = 9.8

@onready var camera: Camera3D = $CameraMount/Camera3D
@onready var camera_mount: Node3D = $CameraMount
@onready var hud: CanvasLayer = $HUD
@onready var health_label: Label = $HUD/HealthLabel
@onready var hit_marker: TextureRect = $HUD/HitMarker

var health: int = 100
var _hit_marker_timer: float = 0.0
var _swing_cooldown: float = 0.0
var _stick: MeshInstance3D = null
var _stick_rest_rotation: Vector3 = Vector3(0.0, PI / 2.0, 0.0)
var _dead := false
var _respawn_timer := 0.0
var _death_screen: CanvasLayer = null
var _respawn_label: Label = null

func _ready() -> void:
	Input.set_mouse_mode(Input.MOUSE_MODE_CAPTURED)
	NetworkManager.register_local_player(self)
	_spawn_stick()
	_spawn_death_screen()

func _spawn_stick() -> void:
	var mesh = load("res://assets/resource-wood.obj")
	_stick = MeshInstance3D.new()
	_stick.mesh = mesh
	_stick.position = Vector3(0.4, -0.25, -0.3)
	_stick.rotation = Vector3(0.0, PI / 2.0, 0.0)
	_stick.scale = Vector3(1.5, 1.5, 1.5)
	var mat = StandardMaterial3D.new()
	mat.albedo_color = Color(0.55, 0.35, 0.15)
	mat.shading_mode = BaseMaterial3D.SHADING_MODE_PER_PIXEL
	_stick.material_override = mat
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
	if _dead:
		_respawn_timer -= delta
		_respawn_label.text = "YOU DIED\nRespawning in %d..." % ceili(_respawn_timer)
		if _respawn_timer <= 0:
			_do_respawn()
		return
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
	tween.tween_property(_stick, "rotation", Vector3(0.0, PI / 2.0, -1.0), 0.1)
	tween.tween_property(_stick, "rotation", _stick_rest_rotation, 0.15)
	
func _spawn_death_screen() -> void:
	_death_screen = CanvasLayer.new()
	_death_screen.visible = false
	add_child(_death_screen)
	var panel = ColorRect.new()
	panel.color = Color(0, 0, 0, 0.6)
	panel.set_anchors_preset(Control.PRESET_FULL_RECT)
	_death_screen.add_child(panel)
	_respawn_label = Label.new()
	_respawn_label.set_anchors_preset(Control.PRESET_CENTER)
	_respawn_label.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	_respawn_label.add_theme_font_size_override("font_size", 32)
	_respawn_label.text = "YOU DIED\nRespawning in 5..."
	_death_screen.add_child(_respawn_label)

func update_health(new_health: int) -> void:
	health = new_health
	health_label.text = "HP: %d" % health

func on_death() -> void:
	_dead = true
	_respawn_timer = 5.0
	_death_screen.visible = true
	_respawn_label.text = "YOU DIED\nRespawning in 5..."
	
func _do_respawn() -> void:
	_dead = false
	_death_screen.visible = false
	health = 100
	health_label.text = "HP: 100"
	global_position = Vector3(0, 1, 0)
	velocity = Vector3.ZERO
	Input.set_mouse_mode(Input.MOUSE_MODE_CAPTURED)
	NetworkManager.send_respawn_request()
