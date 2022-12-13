use bevy::{
    input::mouse::{MouseScrollUnit, MouseWheel},
    prelude::*,
    render::camera::ScalingMode,
    sprite::Anchor,
    window::PresentMode,
};
use bevy_prototype_lyon::prelude::*;

const ASPECT_RATIO: f32 = 16.0 / 9.0;
const CLEAR_COLOR: Color = Color::rgb(0.1, 0.1, 0.1);

#[derive(Resource)]
struct MiningPlatformSprite(Handle<Image>);

fn spawn_camera(mut commands: Commands) {
    let mut camera = Camera2dBundle::default();

    camera.projection.top = 1.0;
    camera.projection.bottom = -1.0;
    camera.projection.right = 1.0 * ASPECT_RATIO;
    camera.projection.left = -1.0 * ASPECT_RATIO;

    camera.projection.scaling_mode = ScalingMode::None;

    commands.spawn(camera);
}

fn spawn_platform(mut commands: Commands, sprite: Res<MiningPlatformSprite>) {
    commands.spawn(SpriteBundle {
        texture: sprite.0.clone(),
        sprite: Sprite {
            custom_size: Some(Vec2::from((48.0, 44.0))),
            anchor: Anchor::Custom(Vec2::from((0.5 / 12.0, -1.5 / 11.0))),
            ..default()
        },
        ..default()
    });
    // For visualizing vehicle center on the ground level
    /*
    commands.spawn(GeometryBuilder::build_as(
        &shapes::RegularPolygon {
            sides: 4,
            feature: shapes::RegularPolygonFeature::Radius(0.5),
            ..default()
        },
        DrawMode::Fill(FillMode::color(Color::rgb(1.0, 0.0, 0.0))),
        Transform::from_xyz(0.0, 0.0, 1.0),
    ));
    */
}

fn load_assets(mut commands: Commands, assets: Res<AssetServer>) {
    let mining_platform_sprite = assets.load("mining_platform.png");
    commands.insert_resource(MiningPlatformSprite(mining_platform_sprite));
}

fn camera_movement(
    mut camera: Query<(&mut OrthographicProjection, &mut Transform), With<Camera2d>>,
    input: Res<Input<KeyCode>>,
    mut mouse_scroll_evr: EventReader<MouseWheel>,
) {
    let (mut camera, mut camera_transform) = camera.single_mut();
    for scroll_event in mouse_scroll_evr.iter() {
        match scroll_event.unit {
            MouseScrollUnit::Line => {
                camera.scale =
                    (camera.scale - 0.5 * scroll_event.y * camera.scale / 10.0).clamp(1.0, 100.0)
            }
            MouseScrollUnit::Pixel => {
                camera.scale =
                    (camera.scale - 0.1 * scroll_event.y * camera.scale / 10.0).clamp(1.0, 100.0)
            }
        }
    }
    let delta = Vec2::from((
        (input.pressed(KeyCode::D) as i8 - input.pressed(KeyCode::A) as i8) as f32,
        (input.pressed(KeyCode::W) as i8 - input.pressed(KeyCode::S) as i8) as f32,
    )) * camera.scale
        / 20.0;
    camera_transform.translation += delta.extend(0.0);
}

fn main() {
    let height = 900.0;
    App::new()
        .insert_resource(ClearColor(CLEAR_COLOR))
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            window: WindowDescriptor {
                title: "Desert Stranding".to_string(),
                present_mode: PresentMode::Fifo,
                height,
                width: height * ASPECT_RATIO,
                resizable: false,
                ..default()
            },
            ..default()
        }))
        .add_plugin(ShapePlugin) // bevy_prototype_lyon
        .add_startup_system_to_stage(StartupStage::PreStartup, load_assets)
        .add_startup_system(spawn_platform)
        .add_startup_system(spawn_camera)
        .add_system(camera_movement)
        .run();
}
