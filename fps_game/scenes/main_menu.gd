extends Control

@onready var cloud_toggle: CheckBox = $CanvasLayer/VBoxContainer/CheckBox
@onready var join_button: Button = $CanvasLayer/VBoxContainer/Button

func _ready() -> void:
	join_button.pressed.connect(_on_join_pressed)

func _on_join_pressed() -> void:
	if cloud_toggle.button_pressed:
		NetworkManager.active_server = NetworkManager.Server.CLOUD
	else:
		NetworkManager.active_server = NetworkManager.Server.TOKYO
	NetworkManager.connect_to_server()
	get_tree().change_scene_to_file("res://scenes/Main.tscn")
