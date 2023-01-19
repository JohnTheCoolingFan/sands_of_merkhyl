#![allow(clippy::too_many_arguments)]

use super::{
    Chunk, ChunkPos, Map, MapPos, MapTilesSprites, Npc, PlayerVehicle, TileKind, TileVisibility,
    WorldSeed,
};
use bevy::{
    math::Vec3Swizzles,
    prelude::*,
    utils::{FloatOrd, HashMap, HashSet},
};
use bevy_ecs_tilemap::{helpers::hex_grid::offset::RowEvenPos, prelude::*};
use rand::prelude::*;
use rangemap::RangeMap;

// Test and adjust
const PLAYER_CHUNK_LOAD_DISTANCE: i32 = 3;
const PLAYER_CHUNK_UNLOAD_DISTANCE: i32 = 5;
const NPC_CHUNK_LOAD_DISTANCE: i32 = 1;
const NPC_CHUNK_UNLOAD_DISTANCE: i32 = 2;
const CAMERA_CHUNK_LOAD_DISTANCE: i32 = 5;
const CAMERA_CHUNK_UNLOAD_DISTANCE: i32 = 10;

const TILEMAP_CHUNK_SIZE: TilemapSize = TilemapSize { x: 32, y: 32 };
const TILEMAP_TILE_SIZE: TilemapTileSize = TilemapTileSize { x: 28.0, y: 32.0 };
const TILEMAP_GRID_SIZE: TilemapGridSize = TilemapGridSize { x: 28.0, y: 32.0 };
const TILEMAP_TYPE: TilemapType = TilemapType::Hexagon(HexCoordSystem::RowEven);

pub struct ChunkManagementPlugin;

impl Plugin for ChunkManagementPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(LoadedChunks::default())
            .insert_resource(LoadedVisibleChunks::default())
            .insert_resource(GeneratedChunks::default())
            .add_system(load_chunks_player)
            .add_system(load_chunks_camera.after(load_chunks_player))
            .add_system(load_chunks_npc.after(load_chunks_camera))
            .add_system(chunk_unload.after(load_chunks_npc));
    }
}

/// Chunks loaded by anything. Chunks not loaded by a player should not be rendered to avoid seeing
/// where npcs are
#[derive(Resource, Default)]
struct LoadedChunks(HashSet<ChunkPos>);

#[derive(Resource, Default)]
struct LoadedVisibleChunks(HashSet<ChunkPos>);

#[derive(Resource, Debug, Clone, Default)]
struct GeneratedChunks {
    chunks: HashMap<ChunkPos, [[TileKind; 32]; 32]>,
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
    chunk_seed[24..28].copy_from_slice(&chunk_pos.x.to_le_bytes());
    chunk_seed[28..32].copy_from_slice(&chunk_pos.y.to_le_bytes());
    let mut rng = SmallRng::from_seed(chunk_seed);
    let generated_values: [[f32; 32]; 32] = rng.gen();
    let rangemap = rangemap_from_weights(vec![(TileKind::Empty, 90.0), (TileKind::Village, 10.0)]);
    generated_values.map(|row| row.map(|v| *rangemap.get(&FloatOrd(v)).unwrap()))
}

pub fn chunk_and_local_from_global(global_pos: RowEvenPos) -> (ChunkPos, TilePos) {
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

pub fn global_from_chunk_and_local(chunk: IVec2, local: TilePos) -> RowEvenPos {
    RowEvenPos {
        q: chunk.x * TILEMAP_CHUNK_SIZE.x as i32 + local.x as i32,
        r: chunk.y * TILEMAP_CHUNK_SIZE.y as i32 + local.y as i32,
    }
}

pub fn chunk_in_world_position(pos: ChunkPos) -> Vec2 {
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

pub fn chunk_center_position(pos: ChunkPos) -> Vec2 {
    let origin_pos = chunk_in_world_position(pos);
    origin_pos + get_tilemap_center(&TILEMAP_CHUNK_SIZE, &TILEMAP_GRID_SIZE, &TILEMAP_TYPE)
}

pub fn camera_to_chunk_pos(camera_pos: Vec2) -> ChunkPos {
    let global_pos = RowEvenPos::from_world_pos(&camera_pos, &TILEMAP_GRID_SIZE);
    chunk_and_local_from_global(global_pos).0
}

pub fn is_chunk_in_radius(origin: ChunkPos, target: ChunkPos, radius: i32) -> bool {
    ((origin.x - radius)..=(origin.x + radius)).contains(&target.x)
        && ((origin.y - radius)..=(origin.y + radius)).contains(&target.y)
}

fn spawn_chunk(
    commands: &mut Commands,
    texture_handle: &Handle<Image>,
    pos: ChunkPos,
    visible: bool,
    chunk_data: &[[TileKind; 32]; 32],
) -> Entity {
    let mut tile_storage = TileStorage::empty(TILEMAP_CHUNK_SIZE);
    let tilemap_entity = commands.spawn_empty().id();
    let tilemap_id = TilemapId(tilemap_entity);

    for x in 0..TILEMAP_CHUNK_SIZE.x {
        for y in 0..TILEMAP_CHUNK_SIZE.y {
            let pos = TilePos { x, y };
            let tile_entity = commands
                .spawn((
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
                    chunk_data[x as usize][y as usize]
                ))
                .id();
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

fn load_chunks_player(
    mut commands: Commands,
    player_vehicles: Query<&MapPos, With<PlayerVehicle>>,
    mut loaded_chunks: ResMut<LoadedChunks>,
    mut visible_chunks: ResMut<LoadedVisibleChunks>,
    map_tile_texture: Res<MapTilesSprites>,
    map_entity: Query<Entity, With<Map>>,
    mut generated_chunks: ResMut<GeneratedChunks>,
    world_seed: Res<WorldSeed>,
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
                    let chunk_data = generated_chunks
                        .chunks
                        .entry(chunk_pos)
                        .or_insert_with(|| generate_chunk(&world_seed.seed, chunk_pos));
                    let chunk_entity = spawn_chunk(
                        &mut commands,
                        &map_tile_texture.0,
                        chunk_pos,
                        true,
                        chunk_data,
                    );
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
    mut generated_chunks: ResMut<GeneratedChunks>,
    world_seed: Res<WorldSeed>,
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
                let chunk_data = generated_chunks
                    .chunks
                    .entry(chunk_pos)
                    .or_insert_with(|| generate_chunk(&world_seed.seed, chunk_pos));
                let chunk_entity = spawn_chunk(
                    &mut commands,
                    &map_tile_texture.0,
                    chunk_pos,
                    true,
                    chunk_data,
                );
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
    mut generated_chunks: ResMut<GeneratedChunks>,
    world_seed: Res<WorldSeed>,
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
                    let chunk_data = generated_chunks
                        .chunks
                        .entry(chunk_pos)
                        .or_insert_with(|| generate_chunk(&world_seed.seed, chunk_pos));
                    let chunk_entity = spawn_chunk(
                        &mut commands,
                        &map_tile_texture.0,
                        chunk_pos,
                        false,
                        chunk_data,
                    );
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
        let camera_chunk_position = camera_to_chunk_pos(camera.single().translation.xy());
        let mut npcs_chunk_positions = npcs.iter().map(|mp| chunk_and_local_from_global(mp.pos).0);
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
