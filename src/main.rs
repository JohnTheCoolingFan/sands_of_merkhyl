#![allow(clippy::type_complexity)]

use bevy::{
    input::mouse::{MouseScrollUnit, MouseWheel},
    prelude::*,
    render::camera::ScalingMode,
    sprite::Anchor,
    utils::FloatOrd,
    window::PresentMode,
};
use bevy_ecs_tilemap::{
    helpers::hex_grid::neighbors::HexRowDirection,
    prelude::{offset::RowEvenPos, *},
};
use bevy_prototype_lyon::prelude::*;
use chunk_management::ChunkManagementPlugin;
use rand::prelude::*;
use rangemap::map::RangeMap;

mod chunk_management;

const ASPECT_RATIO: f32 = 16.0 / 9.0;
const WINDOW_HEIGHT: f32 = 900.0;

const CLEAR_COLOR: Color = Color::rgb(0.1, 0.1, 0.1);
const VISIBLE_TILE_COLOR: TileColor = TileColor(Color::rgb(1.0, 1.0, 1.0));
const CHARTED_TILE_COLOR: TileColor = TileColor(Color::rgb(0.3, 0.3, 0.3));

const MAP_TILEMAP_Z: f32 = 900.0;

const MAP_VIEW_SCALE: f32 = 30.0;
const PLATFORM_VIEW_SCALE: f32 = 25.0;

type ChunkPos = IVec2;

/// How visible (to player) tile is
#[derive(Component, Debug, Clone, Copy)]
enum TileVisibility {
    Visible,
    Charted,
    Unknown,
}

/// What kind of tile it is
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum TileKind {
    Empty = 1,
    Village = 2,
}

/// Marker struct for chunks
#[derive(Component)]
struct Chunk {
    pos: ChunkPos,
}

#[derive(Resource)]
struct WorldSeed {
    seed: [u8; 32],
}

/// Position on a map, with track of how much progress is made through the map tile and what the
/// next tile should be
#[derive(Component, Debug, Clone, PartialEq)]
struct MapPos {
    pos: RowEvenPos,
    current_direction: HexRowDirection,
    target_direction: Option<HexRowDirection>,
    reverse: bool,
    progress: f32,
}

impl Default for MapPos {
    fn default() -> Self {
        Self {
            pos: RowEvenPos { q: 0, r: 0 },
            current_direction: HexRowDirection::East,
            target_direction: None,
            reverse: false,
            progress: 0.5,
        }
    }
}

/// Specifies how something can move on a map
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MovementConstraints {
    /// No limitations, can go to any neighbouring tile, ignores reverse
    Free,
    /// Can only go forward, left-forward or right-forward, or backwards when reversed
    Platform,
}

/// Used for pathfinding
#[derive(Debug, Clone, PartialEq, Eq)]
struct PathfindingPos {
    pos: RowEvenPos,
    direction: HexRowDirection,
    reverse: bool,
}

impl PathfindingPos {
    fn successors(&self, constraints: MovementConstraints) -> Vec<(Self, u32)> {
        todo!()
    }
}

/// Mining platform sprite
#[derive(Resource)]
struct MiningPlatformSprite(Handle<Image>);

/// Sptitesheet of map tiles
#[derive(Resource)]
struct MapTilesSprites(Handle<Image>);

/// Current view mode, map is hiddent when viewing the world
#[derive(Resource)]
enum CurrentView {
    Platform,
    Map,
}

impl CurrentView {
    fn toggle(&mut self) {
        *self = match self {
            Self::Platform => Self::Map,
            Self::Map => Self::Platform,
        }
    }
}

/// Marker struct for Map entity that holds all chunks
#[derive(Component)]
struct Map;

#[derive(Component)]
struct MiningPlatform;

#[derive(Component)]
struct PlayerVehicle;

#[derive(Component)]
struct Npc;

#[derive(Component)]
struct PlayerMapMarker;

#[derive(Component)]
struct PlayerDirectionMapMarker;

fn spawn_camera(mut commands: Commands) {
    let mut camera = Camera2dBundle::default();

    camera.projection.top = 1.0;
    camera.projection.bottom = -1.0;
    camera.projection.right = 1.0 * ASPECT_RATIO;
    camera.projection.left = -1.0 * ASPECT_RATIO;

    camera.projection.scale = PLATFORM_VIEW_SCALE;
    camera.transform.translation.y = 6.0;

    camera.projection.scaling_mode = ScalingMode::None;

    commands.spawn(camera);
}

fn rangemap_from_weights(weights: Vec<(TileKind, f32)>) -> RangeMap<FloatOrd, TileKind> {
    let weights_sum: f32 = weights.iter().map(|(_, w)| w).sum();
    let mut weight_so_far: f32 = 0.0;
    weights
        .into_iter()
        .map(|(k, w)| {
            let range =
                FloatOrd(weight_so_far / weights_sum)..FloatOrd((weight_so_far + w) / weights_sum);
            weight_so_far += w;
            (range, k)
        })
        .collect()
}

fn generate_chunk(world_seed: &[u8; 32], chunk_pos: ChunkPos) -> [[TileKind; 32]; 32] {
    let mut chunk_seed = *world_seed;
    chunk_seed[24..29].copy_from_slice(&chunk_pos.x.to_le_bytes());
    chunk_seed[28..32].copy_from_slice(&chunk_pos.y.to_le_bytes());
    let mut rng = SmallRng::from_seed(chunk_seed);
    let generated_values: [[f32; 32]; 32] = rng.gen();
    let rangemap = rangemap_from_weights(vec![(TileKind::Empty, 90.0), (TileKind::Village, 10.0)]);
    generated_values.map(|row| row.map(|v| *rangemap.get(&FloatOrd(v)).unwrap()))
}

fn spawn_platform(mut commands: Commands, sprite: Res<MiningPlatformSprite>) {
    commands.spawn((
        SpriteBundle {
            texture: sprite.0.clone(),
            sprite: Sprite {
                custom_size: Some(Vec2::from((48.0, 44.0))),
                anchor: Anchor::Custom(Vec2::from((0.5 / 12.0, -1.5 / 11.0))),
                ..default()
            },
            ..default()
        },
        MapPos::default(),
        MiningPlatform,
        PlayerVehicle,
    ));
    // For visualizing vehicle center on the ground level
    /*
    commands.spawn(GeometryBuilder::build_as(
        &shapes::RegularPolygon {
            sides: 4,
            feature: shapes::RegularPolygonFeature::Radius(0.5),
            ..default()
        },
        DrawMode::Fill(FillMode::color(Color::rgb(1.0, 0.0, 0.0))),
        Transform::from_xyz(0.0, 0.0, 950.0),
    ));
    */
}

fn spawn_map(mut commands: Commands) {
    let player_marker = commands
        .spawn((
            PlayerMapMarker,
            GeometryBuilder::build_as(
                &shapes::RegularPolygon {
                    sides: 3,
                    feature: shapes::RegularPolygonFeature::Radius(8.0),
                    ..default()
                },
                DrawMode::Fill(FillMode::color(Color::rgb(0.0, 1.0, 0.0))),
                Transform::from_xyz(0.0, 0.0, 10.0),
            ),
        ))
        .id();

    commands
        .spawn((
            Map,
            TransformBundle::from_transform(Transform::from_xyz(0.0, 0.0, MAP_TILEMAP_Z)),
            VisibilityBundle {
                visibility: Visibility { is_visible: false },
                ..default()
            },
        ))
        //.push_children(&chunks)
        .add_child(player_marker);
}

fn update_map_tiles_texture(
    mut tiles: Query<
        (
            &mut TileTextureIndex,
            &mut TileColor,
            &TileVisibility,
            &TileKind,
        ),
        Or<(Changed<TileVisibility>, Changed<TileKind>)>,
    >,
) {
    for (mut texture_index, mut color, visibility, kind) in tiles.iter_mut() {
        texture_index.0 = if matches!(visibility, TileVisibility::Unknown) {
            0
        } else {
            (*kind as u8).into()
        };
        if matches!(visibility, TileVisibility::Charted) {
            *color = CHARTED_TILE_COLOR
        } else {
            *color = VISIBLE_TILE_COLOR
        };
    }
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

fn switch_view(
    input: Res<Input<KeyCode>>,
    mut camera: Query<(&mut OrthographicProjection, &mut Transform), With<Camera2d>>,
    mut map: Query<&mut Visibility, (With<Map>, Without<Camera2d>)>,
    mut current_view: ResMut<CurrentView>,
) {
    if input.just_pressed(KeyCode::M) {
        current_view.toggle();
        match *current_view {
            CurrentView::Map => {
                map.single_mut().is_visible = true;
                let (mut projection, mut cam_transform) = camera.single_mut();
                projection.scale = MAP_VIEW_SCALE;
                cam_transform.translation = Vec2::new(0.0, 0.0).extend(cam_transform.translation.z);
            }
            CurrentView::Platform => {
                map.single_mut().is_visible = false;
                let (mut projection, mut cam_transform) = camera.single_mut();
                projection.scale = PLATFORM_VIEW_SCALE;
                cam_transform.translation = Vec2::new(0.0, 6.0).extend(cam_transform.translation.z);
            }
        }
    }
}

fn load_assets(mut commands: Commands, assets: Res<AssetServer>) {
    let mining_platform_sprite = assets.load("mining_platform.dds");
    commands.insert_resource(MiningPlatformSprite(mining_platform_sprite));
    let tile_texture = assets.load("map_tiles.dds");
    commands.insert_resource(MapTilesSprites(tile_texture));
}

fn generate_world_seed(mut commands: Commands) {
    let mut seed: [u8; 32] = thread_rng().gen();
    seed[(32 - 8)..].copy_from_slice(&[0; 8]);
    println!("World seed is {:02X?}", seed);
    commands.insert_resource(WorldSeed { seed });
}

fn main() {
    App::new()
        .insert_resource(ClearColor(CLEAR_COLOR))
        .insert_resource(CurrentView::Platform)
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    window: WindowDescriptor {
                        title: "Desert Stranding".to_string(),
                        present_mode: PresentMode::Fifo,
                        height: WINDOW_HEIGHT,
                        width: WINDOW_HEIGHT * ASPECT_RATIO,
                        resizable: false,
                        ..default()
                    },
                    ..default()
                })
                .set(ImagePlugin::default_nearest()),
        )
        .add_plugin(ShapePlugin) // bevy_prototype_lyon
        .add_plugin(TilemapPlugin)
        .add_plugin(ChunkManagementPlugin)
        .add_startup_system_to_stage(StartupStage::PreStartup, load_assets)
        .add_startup_system(generate_world_seed)
        .add_startup_system(spawn_platform)
        .add_startup_system(spawn_camera)
        .add_startup_system(spawn_map)
        .add_system(camera_movement)
        .add_system(switch_view)
        .add_system(update_map_tiles_texture)
        .run();
}
