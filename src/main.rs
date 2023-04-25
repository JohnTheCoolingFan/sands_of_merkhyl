#![allow(clippy::type_complexity)]

use bevy::{
    input::mouse::{MouseScrollUnit, MouseWheel},
    prelude::*,
    render::camera::ScalingMode,
    sprite::Anchor,
    window::{PresentMode, WindowResolution},
};
use bevy_ecs_tilemap::{
    helpers::hex_grid::neighbors::HexRowDirection,
    prelude::{offset::RowEvenPos, *},
};
use bevy_prototype_lyon::prelude::*;
use chunk_management::{global_from_chunk_and_local, ChunkManagementPlugin};
use rand::prelude::*;

mod chunk_management;

use chunk_management::TILEMAP_GRID_SIZE;

const ASPECT_RATIO: f32 = 16.0 / 9.0;
const WINDOW_HEIGHT: f32 = 900.0;

const CLEAR_COLOR: Color = Color::rgb(0.1, 0.1, 0.1);
const VISIBLE_TILE_COLOR: TileColor = TileColor(Color::rgb(1.0, 1.0, 1.0));
const CHARTED_TILE_COLOR: TileColor = TileColor(Color::rgb(0.3, 0.3, 0.3));

const MAP_TILEMAP_Z: f32 = 900.0;

const MAP_VIEW_SCALE: f32 = 30.0;
const PLATFORM_VIEW_SCALE: f32 = 25.0;

#[inline]
fn direction_to_rotation(direction: HexRowDirection) -> Quat {
    Quat::from_rotation_z(
        match direction {
            HexRowDirection::North => 0_f32,
            HexRowDirection::NorthEast => -30_f32,
            HexRowDirection::NorthWest => 30_f32,
            HexRowDirection::South => -180_f32,
            HexRowDirection::SouthWest => 150_f32,
            HexRowDirection::SouthEast => 210_f32,
        }
        .to_radians(),
    )
}

type ChunkPos = IVec2;

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
struct ChartRange(u32);

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

impl Default for WorldSeed {
    fn default() -> Self {
        let mut seed: [u8; 32] = thread_rng().gen();
        seed[(32 - 8)..].copy_from_slice(&[0; 8]);
        info!("World seed is {:02X?}", seed);
        WorldSeed { seed }
    }
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
            current_direction: HexRowDirection::North,
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

/// Spirtes used in the game
#[derive(Resource)]
struct SpriteAssets {
    /// Mining platform sprite
    mining_platform: Handle<Image>,
    /// Sptitesheet of map tiles
    map_tiles: Handle<Image>,
}

impl FromWorld for SpriteAssets {
    fn from_world(world: &mut World) -> Self {
        let asset_server = world.get_resource::<AssetServer>().unwrap();
        let mining_platform = asset_server.load("mining_platform.dds");
        let map_tiles = asset_server.load("map_tiles.dds");
        Self {
            mining_platform,
            map_tiles,
        }
    }
}

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

    /*
    camera.projection.top = 1.0;
    camera.projection.bottom = -1.0;
    camera.projection.right = 1.0 * ASPECT_RATIO;
    camera.projection.left = -1.0 * ASPECT_RATIO;
    */

    camera.projection.scale = PLATFORM_VIEW_SCALE;
    camera.transform.translation.y = 6.0;

    camera.projection.scaling_mode = ScalingMode::Fixed {
        height: 2.0,
        width: 2.0 * ASPECT_RATIO,
    };

    commands
        .spawn((
            camera,
            VisibilityBundle {
                visibility: Visibility::Hidden,
                computed: ComputedVisibility::default(),
            },
        ))
        .with_children(|cb| {
            cb.spawn((
                ShapeBundle {
                    path: GeometryBuilder::build_as(&shapes::Rectangle {
                        extents: Vec2 {
                            x: 1000.0,
                            y: 1000.0,
                        },
                        origin: RectangleOrigin::Center,
                    }),
                    transform: Transform::from_xyz(0.0, 0.0, -300.0),
                    ..default()
                },
                Fill::color(CLEAR_COLOR),
            ));
        });
}

fn spawn_platform(mut commands: Commands, sprite: Res<SpriteAssets>) {
    commands.spawn((
        SpriteBundle {
            texture: sprite.mining_platform.clone(),
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
        ChartRange(5),
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
    let TransformBundle {
        local: transform,
        global: global_transform,
    } = TransformBundle::from_transform(Transform::from_xyz(0.0, 0.0, 10.0).with_scale(Vec3 {
        x: 0.5,
        y: 1.0,
        z: 1.0,
    }));
    let player_marker = commands
        .spawn((
            PlayerMapMarker,
            ShapeBundle {
                path: GeometryBuilder::build_as(&shapes::RegularPolygon {
                    sides: 3,
                    feature: shapes::RegularPolygonFeature::Radius(8.0),
                    ..default()
                }),
                transform,
                global_transform,
                ..default()
            },
            Fill::color(Color::rgb(0.0, 1.0, 0.0)),
        ))
        .id();

    commands
        .spawn((
            Map,
            TransformBundle::from_transform(Transform::from_xyz(0.0, 0.0, MAP_TILEMAP_Z)),
            VisibilityBundle {
                visibility: Visibility::Hidden,
                ..default()
            },
        ))
        //.push_children(&chunks)
        .add_child(player_marker);
}

fn chart_map(
    player: Query<(&MapPos, &ChartRange), With<PlayerVehicle>>,
    mut tiles: Query<(&mut TileVisibility, &TilePos, &TilemapId)>,
    chunks: Query<&Chunk>,
) {
    let (player_pos, chart_range) = player.single();
    let tiles_in_chart_range: Vec<RowEvenPos> =
        generate_hexagon(player_pos.pos.into(), chart_range.0)
            .into_iter()
            .map(Into::into)
            .collect();
    for (mut tile_vis, tile_pos, tilemap_id) in tiles.iter_mut() {
        let chunk = chunks.get(tilemap_id.0).unwrap();
        let global_tile_pos = global_from_chunk_and_local(chunk.pos, *tile_pos);
        if tiles_in_chart_range.contains(&global_tile_pos) {
            *tile_vis = TileVisibility::Visible
        } else if matches!(*tile_vis, TileVisibility::Visible) {
            *tile_vis = TileVisibility::Charted
        }
    }
}

// Breaks when multiple player vehicles: Does not update. Add a entity id of a player vehicle to
// each marker?
fn update_marker(
    mut marker: Query<&mut Transform, With<PlayerMapMarker>>,
    player: Query<&MapPos, (With<PlayerVehicle>, Changed<MapPos>)>,
) {
    let mut marker_transform = marker.single_mut();
    if let Ok(player_pos) = player.get_single() {
        marker_transform.translation = player_pos
            .pos
            .center_in_world(&TILEMAP_GRID_SIZE)
            .extend(marker_transform.translation.z);
        marker_transform.rotation = direction_to_rotation(player_pos.current_direction);
    }
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
        if matches!(visibility, TileVisibility::Visible) {
            *color = VISIBLE_TILE_COLOR
        } else {
            *color = CHARTED_TILE_COLOR
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
    mut camera: Query<
        (&mut OrthographicProjection, &mut Transform, &mut Visibility),
        With<Camera2d>,
    >,
    mut map: Query<&mut Visibility, (With<Map>, Without<Camera2d>)>,
    mut current_view: ResMut<CurrentView>,
) {
    if input.just_pressed(KeyCode::M) {
        current_view.toggle();
        match *current_view {
            CurrentView::Map => {
                *map.single_mut() = Visibility::Visible;
                let (mut projection, mut cam_transform, mut cam_visibility) = camera.single_mut();
                *cam_visibility = Visibility::Visible;
                projection.scale = MAP_VIEW_SCALE;
                cam_transform.translation = Vec2::new(0.0, 0.0).extend(cam_transform.translation.z);
            }
            CurrentView::Platform => {
                *map.single_mut() = Visibility::Hidden;
                let (mut projection, mut cam_transform, mut cam_visibility) = camera.single_mut();
                *cam_visibility = Visibility::Hidden;
                projection.scale = PLATFORM_VIEW_SCALE;
                cam_transform.translation = Vec2::new(0.0, 6.0).extend(cam_transform.translation.z);
            }
        }
    }
}

fn main() {
    App::new()
        .insert_resource(ClearColor(CLEAR_COLOR))
        .insert_resource(CurrentView::Platform)
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "Sun never sets on the sands of Merkhyl".to_string(),
                        present_mode: PresentMode::Fifo,
                        resolution: WindowResolution::new(
                            WINDOW_HEIGHT * ASPECT_RATIO,
                            WINDOW_HEIGHT,
                        ),
                        resizable: false,
                        ..default()
                    }),
                    ..default()
                })
                .set(ImagePlugin::default_nearest()),
        )
        .add_plugin(ShapePlugin) // bevy_prototype_lyon
        .add_plugin(TilemapPlugin)
        .add_plugin(ChunkManagementPlugin)
        .init_resource::<SpriteAssets>()
        .init_resource::<WorldSeed>()
        .add_startup_system(spawn_platform)
        .add_startup_system(spawn_camera)
        .add_startup_system(spawn_map)
        .add_system(camera_movement)
        .add_system(switch_view)
        .add_system(update_map_tiles_texture)
        .add_system(update_marker)
        .add_system(chart_map)
        .run();
}
