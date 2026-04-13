extends CharacterBody3D
# LocalPlayer.gd
const SPEED = 6.0
const JUMP_VELOCITY = 5.0
const MOUSE_SENSITIVITY = 0.002
const GRAVITY = 9.8

@onready var camera: Camera3D = $CameraMount/Camera3D
@onready var camera_mount: Node3D = $CameraMount
@onready var shoot_raycast: RayCast3D = $CameraMount/Camera3D/ShootRay
@onready var muzzle_flash: GPUParticles3D = $CameraMount/Camera3D/MuzzleFlash
@onready var hud: CanvasLayer = $HUD
@onready var health_label: Label = $HUD/HealthLabel
@onready var crosshair: TextureRect = $HUD/Crosshair
@onready var hit_marker: TextureRect = $HUD/HitMarker

var health: int = 100
var _hit_marker_timer: float = 0.0
var _shoot_cooldown: float = 0.0

func _ready() -> void:
	Input.set_mouse_mode(Input.MOUSE_MODE_CAPTURED)
	# Register ourselves with NetworkManager so it can read our position
	NetworkManager.register_local_player(self)

func _unhandled_input(event: InputEvent) -> void:
	if event is InputEventMouseMotion:
		rotate_y(-event.relative.x * MOUSE_SENSITIVITY)
		camera_mount.rotate_x(-event.relative.y * MOUSE_SENSITIVITY)
		camera_mount.rotation.x = clamp(camera_mount.rotation.x, -PI/2.2, PI/2.2)
	if event.is_action_pressed("shoot") and _shoot_cooldown <= 0.0:
		_do_shoot()
	if event is InputEventKey and event.pressed:
		if event.keycode == KEY_ESCAPE:
			Input.set_mouse_mode(Input.MOUSE_MODE_VISIBLE)
		if event.keycode == KEY_F1:
			Input.set_mouse_mode(Input.MOUSE_MODE_CAPTURED)

func _physics_process(delta: float) -> void:
	_shoot_cooldown -= delta
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
	# NetworkManager reads our position/rotation directly - no call needed here

func _do_shoot() -> void:
	_shoot_cooldown = 0.15
	if muzzle_flash:
		muzzle_flash.restart()
		muzzle_flash.emitting = true
	shoot_raycast.force_raycast_update()
	if shoot_raycast.is_colliding():
		var hit = shoot_raycast.get_collider()
		if hit and hit.is_in_group("remote_players"):
			hit_marker.visible = true
			_hit_marker_timer = 0.3
	# Shoot events not implemented yet - save for next week
