#![allow(clippy::type_complexity)]

use bevy::{
    input::mouse::{MouseScrollUnit, MouseWheel},
    prelude::*,
    render::camera::ScalingMode,
    sprite::Anchor,
    utils::HashSet,
    window::PresentMode,
};
use bevy_ecs_tilemap::prelude::{offset::RowEvenPos, *};
use bevy_prototype_lyon::prelude::*;
use splines::{Interpolation, Key, Spline};

const ASPECT_RATIO: f32 = 16.0 / 9.0;

const CLEAR_COLOR: Color = Color::rgb(0.1, 0.1, 0.1);
const VISIBLE_TILE_COLOR: TileColor = TileColor(Color::rgb(1.0, 1.0, 1.0));
const CHARTED_TILE_COLOR: TileColor = TileColor(Color::rgb(0.3, 0.3, 0.3));

const MAP_TILEMAP_Z: f32 = 900.0;

// Test and adjust
const PLAYER_CHUNK_UNLOAD_DISTANCE: f32 = 100.0;
const PLAYER_CHUNK_LOAD_DISTANCE: i32 = 3;
const NPC_CHUNK_LOAD_DISTANCE: i32 = 1;
const NPC_CHUNK_UNLOAD_DISTANCE: f32 = 30.0;
const CAMERA_CHUNK_LOAD_DISTANCE: i32 = 5;
const CAMERA_CHUNK_UNLOAD_DISTANCE: f32 = 100.0;

const MAP_VIEW_SCALE: f32 = 1.0;
const PLATFORM_VIEW_SCALE: f32 = 25.0;

const TILEMAP_CHUNK_SIZE: TilemapSize = TilemapSize { x: 32, y: 32 };
const TILEMAP_TILE_SIZE: TilemapTileSize = TilemapTileSize { x: 28.0, y: 32.0 };
const TILEMAP_GRID_SIZE: TilemapGridSize = TilemapGridSize { x: 28.0, y: 32.0 };
const TILEMAP_TYPE: TilemapType = TilemapType::Hexagon(HexCoordSystem::RowEven);

type ChunkPos = IVec2;

/// How visible (to player) tile is
#[derive(Component, Debug, Clone, Copy)]
enum TileVisibility {
    Visible,
    Charted,
    Unknown,
}

/// What kind of tile it is
#[derive(Component, Debug, Clone, Copy)]
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

/// Chunks loaded solely by a player, whould be visible
#[derive(Resource)]
struct PlayerLoadedChunks(HashSet<ChunkPos>);

/// Chunks loaded by a camera
#[derive(Resource)]
struct CameraLoadedChunks(HashSet<ChunkPos>);

/// Chunks loaded by anything. Chunks not loaded by a player should not be rendered to avoid seeing
/// where npcs are
#[derive(Resource)]
struct LoadedChunks(HashSet<ChunkPos>);

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

fn chunk_and_local_from_global(global_pos: RowEvenPos) -> (ChunkPos, TilePos) {
    let chunk_pos = ChunkPos::new(
        global_pos.q.div_euclid(TILEMAP_CHUNK_SIZE.x as i32),
        global_pos.r.div_euclid(TILEMAP_CHUNK_SIZE.y as i32),
    );
    let tile_pos = TilePos {
        x: global_pos.q.rem_euclid(TILEMAP_CHUNK_SIZE.x as i32) as u32,
        y: global_pos.r.rem_euclid(TILEMAP_CHUNK_SIZE.y as i32) as u32,
    };
    (chunk_pos, tile_pos)
}

fn global_from_chunk_and_local(chunk: IVec2, local: TilePos) -> RowEvenPos {
    RowEvenPos {
        q: chunk.x * TILEMAP_CHUNK_SIZE.x as i32 + local.x as i32,
        r: chunk.y * TILEMAP_CHUNK_SIZE.y as i32 + local.y as i32,
    }
}

fn chunk_in_world_position(pos: ChunkPos) -> Vec2 {
    Vec2::new(
        TILEMAP_TILE_SIZE.x * TILEMAP_CHUNK_SIZE.x as f32 * pos.x as f32,
        TilePos {
            x: 0,
            y: TILEMAP_CHUNK_SIZE.y,
        }
        .center_in_world(&TILEMAP_GRID_SIZE, &TILEMAP_TYPE)
        .y * pos.y as f32,
    )
}

fn chunk_center_position(pos: ChunkPos) -> Vec2 {
    let origin_pos = chunk_in_world_position(pos);
    origin_pos + get_tilemap_center(&TILEMAP_CHUNK_SIZE, &TILEMAP_GRID_SIZE, &TILEMAP_TYPE)
}

fn camera_to_chunk_pos(camera_pos: Vec2) -> ChunkPos {
    let camera_pos = camera_pos.as_ivec2();
    let chunk_size = IVec2::new(TILEMAP_CHUNK_SIZE.x as i32, TILEMAP_CHUNK_SIZE.y as i32);
    let tile_size = IVec2::new(TILEMAP_TILE_SIZE.x as i32, TILEMAP_TILE_SIZE.y as i32);
    let chunk_size_in_units = chunk_size * tile_size;
    IVec2::new(
        camera_pos.x.div_euclid(chunk_size_in_units.x),
        camera_pos.y.div_euclid(chunk_size_in_units.y),
    )
}

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

fn spline_from_weights(weights: Vec<(TileKind, f32)>) -> Spline<f32, f32> {
    let weights_sum: f32 = weights.iter().map(|(_, w)| w).sum();
    let mut weight_so_far: f32 = 0.0;
    let mut keys: Vec<_> = weights
        .into_iter()
        .map(|(k, w)| {
            let value = (k as u8) as f32;
            let key_value = weight_so_far / weights_sum;
            weight_so_far += w;
            Key {
                t: key_value,
                value,
                interpolation: Interpolation::Step(1.0),
            }
        })
        .collect();
    keys.push(Key {
        t: 1.0,
        value: 0.0,
        interpolation: Interpolation::default(),
    });
    Spline::from_vec(keys)
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
        Transform::from_xyz(0.0, 0.0, 950.0),
    ));
    */
}

fn spawn_map(
    mut commands: Commands,
    texture_handle: Res<MapTilesSprites>,
    mut rendered_chunks: ResMut<LoadedChunks>,
) {
    let mut chunks = Vec::new();

    for x in 0..3 {
        for y in 0..3 {
            let pos = IVec2::new(x, y);
            chunks.push(spawn_chunk(&mut commands, &texture_handle.0, pos, true));
            rendered_chunks.0.insert(pos);
        }
    }

    commands
        .spawn((
            Map,
            TransformBundle::from_transform(Transform::from_xyz(0.0, 0.0, MAP_TILEMAP_Z)),
            VisibilityBundle {
                visibility: Visibility { is_visible: false },
                ..default()
            },
        ))
        .push_children(&chunks);
}

fn spawn_chunk(
    commands: &mut Commands,
    texture_handle: &Handle<Image>,
    pos: ChunkPos,
    visible: bool,
) -> Entity {
    let mut tile_storage = TileStorage::empty(TILEMAP_CHUNK_SIZE);
    let tilemap_entity = commands.spawn_empty().id();
    let tilemap_id = TilemapId(tilemap_entity);

    for x in 0..TILEMAP_CHUNK_SIZE.x {
        for y in 0..TILEMAP_CHUNK_SIZE.y {
            let pos = TilePos { x, y };
            let tile_entity = commands.spawn((
                TileBundle {
                    position: pos,
                    texture_index: TileTextureIndex(0), // TODO,
                    tilemap_id,
                    visible: TileVisible(true),
                    flip: TileFlip::default(),
                    color: TileColor::default(),
                    old_position: TilePosOld::default(),
                },
                TileVisibility::Visible, // TODO
                TileKind::Empty,
            ));
            tile_storage.set(&pos, tile_entity.id());
        }
    }

    commands.entity(tilemap_entity).insert((
        TilemapBundle {
            grid_size: TILEMAP_GRID_SIZE,
            size: TILEMAP_CHUNK_SIZE,
            storage: tile_storage,
            texture: TilemapTexture::Single(texture_handle.clone()),
            tile_size: TILEMAP_TILE_SIZE,
            map_type: TILEMAP_TYPE,
            transform: Transform::from_translation(chunk_in_world_position(pos).extend(0.0)),
            visibility: Visibility {
                is_visible: visible,
            },
            ..default()
        },
        Chunk { pos },
    ));

    tilemap_entity
}

fn update_texture(
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
    let mining_platform_sprite = assets.load("mining_platform.png");
    commands.insert_resource(MiningPlatformSprite(mining_platform_sprite));
    let tile_texture = assets.load("map_tiles.png");
    commands.insert_resource(MapTilesSprites(tile_texture));
}

fn main() {
    let height = 900.0;
    App::new()
        .insert_resource(ClearColor(CLEAR_COLOR))
        .insert_resource(CurrentView::Platform)
        .insert_resource(LoadedChunks(HashSet::new()))
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    window: WindowDescriptor {
                        title: "Desert Stranding".to_string(),
                        present_mode: PresentMode::Fifo,
                        height,
                        width: height * ASPECT_RATIO,
                        resizable: false,
                        ..default()
                    },
                    ..default()
                })
                .set(ImagePlugin::default_nearest()),
        )
        .add_plugin(ShapePlugin) // bevy_prototype_lyon
        .add_plugin(TilemapPlugin)
        .add_startup_system_to_stage(StartupStage::PreStartup, load_assets)
        .add_startup_system(spawn_platform)
        .add_startup_system(spawn_camera)
        .add_startup_system(spawn_map)
        .add_system(camera_movement)
        .add_system(switch_view)
        .add_system(update_texture)
        .run();
}
