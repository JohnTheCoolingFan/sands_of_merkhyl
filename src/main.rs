#![allow(clippy::type_complexity)]

use bevy::{
    input::mouse::{MouseScrollUnit, MouseWheel},
    math::Vec3Swizzles,
    prelude::*,
    render::camera::ScalingMode,
    sprite::Anchor,
    utils::HashSet,
    window::PresentMode,
};
use bevy_ecs_tilemap::{
    helpers::hex_grid::neighbors::HexRowDirection,
    prelude::{offset::RowEvenPos, *},
};
use bevy_prototype_lyon::prelude::*;
use splines::{Interpolation, Key, Spline};

const ASPECT_RATIO: f32 = 16.0 / 9.0;
const WINDOW_HEIGHT: f32 = 900.0;

const CLEAR_COLOR: Color = Color::rgb(0.1, 0.1, 0.1);
const VISIBLE_TILE_COLOR: TileColor = TileColor(Color::rgb(1.0, 1.0, 1.0));
const CHARTED_TILE_COLOR: TileColor = TileColor(Color::rgb(0.3, 0.3, 0.3));

const MAP_TILEMAP_Z: f32 = 900.0;

// Test and adjust
const PLAYER_CHUNK_LOAD_DISTANCE: i32 = 3;
const PLAYER_CHUNK_UNLOAD_DISTANCE: i32 = 5;
const NPC_CHUNK_LOAD_DISTANCE: i32 = 1;
const NPC_CHUNK_UNLOAD_DISTANCE: i32 = 2;
const CAMERA_CHUNK_LOAD_DISTANCE: i32 = 5;
const CAMERA_CHUNK_UNLOAD_DISTANCE: i32 = 10;

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

/// Chunks loaded by anything. Chunks not loaded by a player should not be rendered to avoid seeing
/// where npcs are
#[derive(Resource, Default)]
struct LoadedChunks(HashSet<ChunkPos>);

#[derive(Resource, Default)]
struct LoadedVisibleChunks(HashSet<ChunkPos>);

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
    let global_pos = RowEvenPos::from_world_pos(&camera_pos, &TILEMAP_GRID_SIZE);
    chunk_and_local_from_global(global_pos).0
}

fn is_chunk_in_radius(origin: ChunkPos, target: ChunkPos, radius: i32) -> bool {
    ((origin.x - radius)..=(origin.x + radius)).contains(&target.x)
        && ((origin.y - radius)..=(origin.y + radius)).contains(&target.y)
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

fn spawn_map(
    mut commands: Commands,
    texture_handle: Res<MapTilesSprites>,
    mut loaded_chunks: ResMut<LoadedChunks>,
) {
    /*
    let mut chunks = Vec::new();

    for x in 0..3 {
        for y in 0..3 {
            let pos = IVec2::new(x, y);
            chunks.push(spawn_chunk(&mut commands, &texture_handle.0, pos, true));
            loaded_chunks.0.insert(pos);
        }
    }
    */

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
            )).id();
            commands.entity(tilemap_entity).add_child(tile_entity);
            tile_storage.set(&pos, tile_entity);
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

fn load_chunks_player(
    mut commands: Commands,
    player_vehicles: Query<&MapPos, With<PlayerVehicle>>,
    mut loaded_chunks: ResMut<LoadedChunks>,
    mut visible_chunks: ResMut<LoadedVisibleChunks>,
    map_tile_texture: Res<MapTilesSprites>,
    map_entity: Query<Entity, With<Map>>,
) {
    let map_entity = map_entity.single();
    for player_pos in player_vehicles.iter() {
        let player_chunk_pos = chunk_and_local_from_global(player_pos.pos).0;
        for x in (player_chunk_pos.x - PLAYER_CHUNK_LOAD_DISTANCE)
            ..=(player_chunk_pos.x + PLAYER_CHUNK_LOAD_DISTANCE)
        {
            for y in (player_chunk_pos.y - PLAYER_CHUNK_LOAD_DISTANCE)
                ..=(player_chunk_pos.y + PLAYER_CHUNK_LOAD_DISTANCE)
            {
                let chunk_pos = IVec2::new(x, y);
                if !loaded_chunks.0.contains(&chunk_pos) {
                    let chunk_entity =
                        spawn_chunk(&mut commands, &map_tile_texture.0, chunk_pos, true);
                    loaded_chunks.0.insert(chunk_pos);
                    let mut map_entity_commands = commands.entity(map_entity);
                    map_entity_commands.add_child(chunk_entity);
                }
                if !visible_chunks.0.contains(&chunk_pos) {
                    visible_chunks.0.insert(chunk_pos);
                }
            }
        }
    }
}

fn load_chunks_camera(
    mut commands: Commands,
    camera: Query<&Transform, With<Camera2d>>,
    mut loaded_chunks: ResMut<LoadedChunks>,
    mut visible_chunks: ResMut<LoadedVisibleChunks>,
    map_tile_texture: Res<MapTilesSprites>,
    map_entity: Query<Entity, With<Map>>,
) {
    let map_entity = map_entity.single();
    let camera_transform = camera.single();
    let camera_chunk_pos = camera_to_chunk_pos(camera_transform.translation.xy());
    for x in (camera_chunk_pos.x - CAMERA_CHUNK_LOAD_DISTANCE)
        ..=(camera_chunk_pos.x + CAMERA_CHUNK_LOAD_DISTANCE)
    {
        for y in (camera_chunk_pos.y - CAMERA_CHUNK_LOAD_DISTANCE)
            ..=(camera_chunk_pos.y + CAMERA_CHUNK_LOAD_DISTANCE)
        {
            let chunk_pos = IVec2::new(x, y);
            if !loaded_chunks.0.contains(&chunk_pos) {
                let chunk_entity = spawn_chunk(&mut commands, &map_tile_texture.0, chunk_pos, true);
                loaded_chunks.0.insert(chunk_pos);
                let mut map_entity_commands = commands.entity(map_entity);
                map_entity_commands.add_child(chunk_entity);
            }
            if !visible_chunks.0.contains(&chunk_pos) {
                visible_chunks.0.insert(chunk_pos);
            }
        }
    }
}

fn load_chunks_npc(
    mut commands: Commands,
    npcs: Query<&MapPos, With<Npc>>,
    mut loaded_chunks: ResMut<LoadedChunks>,
    map_tile_texture: Res<MapTilesSprites>,
    map_entity: Query<Entity, With<Map>>,
) {
    let map_entity = map_entity.single();
    for npc_map_pos in npcs.iter() {
        let npc_chunk_pos = chunk_and_local_from_global(npc_map_pos.pos).0;
        for x in (npc_chunk_pos.x - NPC_CHUNK_LOAD_DISTANCE)
            ..=(npc_chunk_pos.x + NPC_CHUNK_LOAD_DISTANCE)
        {
            for y in (npc_chunk_pos.y - NPC_CHUNK_LOAD_DISTANCE)
                ..=(npc_chunk_pos.y + NPC_CHUNK_LOAD_DISTANCE)
            {
                let chunk_pos = IVec2::new(x, y);
                if !loaded_chunks.0.contains(&chunk_pos) {
                    let chunk_entity =
                        spawn_chunk(&mut commands, &map_tile_texture.0, chunk_pos, false);
                    loaded_chunks.0.insert(chunk_pos);
                    let mut map_entity_commands = commands.entity(map_entity);
                    map_entity_commands.add_child(chunk_entity);
                }
            }
        }
    }
}

fn chunk_unload(
    mut commands: Commands,
    player_vehicles: Query<&MapPos, With<PlayerVehicle>>,
    camera: Query<&Transform, With<Camera2d>>,
    npcs: Query<&MapPos, With<Npc>>,
    chunks: Query<(Entity, &Chunk)>,
    mut loaded_chunks: ResMut<LoadedChunks>,
    mut visible_chunks: ResMut<LoadedVisibleChunks>,
) {
    for (chunk_entity, Chunk { pos: chunk_pos }) in chunks.iter() {
        let mut player_chunk_positions = player_vehicles
            .iter()
            .map(|mp| chunk_and_local_from_global(mp.pos).0);
        let camera_chunk_position =
            camera_to_chunk_pos(camera.single().translation.xy());
        let mut npcs_chunk_positions = npcs
            .iter()
            .map(|mp| chunk_and_local_from_global(mp.pos).0);
        if !(player_chunk_positions
            .any(|p| is_chunk_in_radius(p, *chunk_pos, PLAYER_CHUNK_UNLOAD_DISTANCE))
            || is_chunk_in_radius(
                camera_chunk_position,
                *chunk_pos,
                CAMERA_CHUNK_UNLOAD_DISTANCE,
            ))
        {
            visible_chunks.0.remove(chunk_pos);
        }
        if !(player_chunk_positions
            .any(|p| is_chunk_in_radius(p, *chunk_pos, PLAYER_CHUNK_UNLOAD_DISTANCE))
            || is_chunk_in_radius(
                camera_chunk_position,
                *chunk_pos,
                CAMERA_CHUNK_UNLOAD_DISTANCE,
            )
            || npcs_chunk_positions
                .any(|p| is_chunk_in_radius(p, *chunk_pos, NPC_CHUNK_UNLOAD_DISTANCE)))
        {
            commands.entity(chunk_entity).despawn_recursive();
            loaded_chunks.0.remove(chunk_pos);
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
    App::new()
        .insert_resource(ClearColor(CLEAR_COLOR))
        .insert_resource(CurrentView::Platform)
        .insert_resource(LoadedChunks::default())
        .insert_resource(LoadedVisibleChunks::default())
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
        .add_startup_system_to_stage(StartupStage::PreStartup, load_assets)
        .add_startup_system(spawn_platform)
        .add_startup_system(spawn_camera)
        .add_startup_system(spawn_map)
        .add_system(camera_movement)
        .add_system(switch_view)
        .add_system(update_map_tiles_texture)
        .add_system(load_chunks_player)
        .add_system(load_chunks_camera.after(load_chunks_player))
        .add_system(load_chunks_npc.after(load_chunks_camera))
        .add_system(chunk_unload.after(load_chunks_npc))
        .run();
}
