extends CharacterBody3D

# LocalPlayer.gd
# The player YOU control. Handles input, movement, shooting.
# Sends state to NetworkManager - never touches networking directly.

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
var _input_this_frame: Dictionary = {}

func _ready() -> void:
	Input.set_mouse_mode(Input.MOUSE_MODE_CAPTURED)
	NetworkManager.connect("hit_confirmed", _on_hit_confirmed)
	NetworkManager.connect("player_state_updated", _on_player_state_updated)
	shoot_raycast.enabled = true

func _unhandled_input(event: InputEvent) -> void:
	if event is InputEventMouseMotion:
		# Rotate player body left/right
		rotate_y(-event.relative.x * MOUSE_SENSITIVITY)
		# Rotate camera up/down
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

	# Gravity
	if not is_on_floor():
		velocity.y -= GRAVITY * delta

	# Movement input
	var input_dir = Input.get_vector("move_left", "move_right", "move_forward", "move_backward")
	var direction = (transform.basis * Vector3(input_dir.x, 0, input_dir.y)).normalized()

	if is_on_floor():
		velocity.x = direction.x * SPEED
		velocity.z = direction.z * SPEED
	else:
		velocity.x = lerp(velocity.x, direction.x * SPEED, delta * 3.0)
		velocity.z = lerp(velocity.z, direction.z * SPEED, delta * 3.0)

	var jumped = false
	if Input.is_action_just_pressed("jump") and is_on_floor():
		velocity.y = JUMP_VELOCITY
		jumped = true

	move_and_slide()

	# Send input to network manager every frame
	NetworkManager.send_player_input(
		direction,
		Vector2(rotation.y, camera_mount.rotation.x),
		jumped
	)

func _do_shoot() -> void:
	_shoot_cooldown = 0.15

	if muzzle_flash:
		muzzle_flash.restart()
		muzzle_flash.emitting = true

	# Raycast for hit detection (client-side, server will confirm)
	shoot_raycast.force_raycast_update()
	if shoot_raycast.is_colliding():
		var hit = shoot_raycast.get_collider()
		if hit and hit.is_in_group("remote_players"):
			# Show immediate hit marker (prediction - server confirms)
			hit_marker.visible = true
			_hit_marker_timer = 0.3

	# Always send shoot event to server regardless
	var origin = camera.global_position
	var direction = -camera.global_transform.basis.z
	NetworkManager.send_shoot_event(origin, direction)

func _on_hit_confirmed(_shooter_id: int, target_id: int, _damage: int) -> void:
	# Server confirmed a hit - update UI if we were the target
	if target_id == NetworkManager.local_player_id:
		health -= 10
		health_label.text = "HP: %d" % health

func _on_player_state_updated(player_id: int, state: Dictionary) -> void:
	if player_id == NetworkManager.local_player_id:
		health = state.get("health", health)
		health_label.text = "HP: %d" % health
