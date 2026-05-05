extends Control

@onready var server_select: OptionButton = $CanvasLayer/VBoxContainer/OptionButton
@onready var join_button: Button = $CanvasLayer/VBoxContainer/Button

func _ready() -> void:
	server_select.add_item("Cloud")
	server_select.add_item("Tokyo")
	server_select.add_item("Local")
	join_button.pressed.connect(_on_join_pressed)

func _on_join_pressed() -> void:
	match server_select.selected:
		0: NetworkManager.active_server = NetworkManager.Server.CLOUD
		1: NetworkManager.active_server = NetworkManager.Server.TOKYO
		2: NetworkManager.active_server = NetworkManager.Server.LOCAL
	await NetworkManager.connect_to_server()
	get_tree().change_scene_to_file("res://scenes/Main.tscn")
