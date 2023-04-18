#![allow(clippy::too_many_arguments)]

use crate::SpriteAssets;

use super::{
    Chunk, ChunkPos, Map, MapPos, Npc, PlayerVehicle, TileKind, TileVisibility, WorldSeed,
};
use bevy::{
    prelude::*,
    utils::{HashMap, HashSet},
};
use bevy_ecs_tilemap::{helpers::hex_grid::offset::RowEvenPos, prelude::*};
use rand::{distributions::WeightedIndex, prelude::*};

// Test and adjust
const PLAYER_CHUNK_LOAD_DISTANCE: i32 = 3;
const PLAYER_CHUNK_UNLOAD_DISTANCE: i32 = 5;
const NPC_CHUNK_LOAD_DISTANCE: i32 = 1;
const NPC_CHUNK_UNLOAD_DISTANCE: i32 = 2;

pub const TILEMAP_CHUNK_SIZE: TilemapSize = TilemapSize { x: 32, y: 32 };
pub const TILEMAP_TILE_SIZE: TilemapTileSize = TilemapTileSize { x: 28.0, y: 32.0 };
pub const TILEMAP_GRID_SIZE: TilemapGridSize = TilemapGridSize { x: 28.0, y: 32.0 };
pub const TILEMAP_TYPE: TilemapType = TilemapType::Hexagon(HexCoordSystem::RowEven);

pub struct ChunkManagementPlugin;

impl Plugin for ChunkManagementPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(LoadedChunks::default())
            .insert_resource(GeneratedChunks::default())
            .add_system(load_chunks_player)
            .add_system(load_chunks_npc.after(load_chunks_player))
            .add_system(chunk_unload.after(load_chunks_npc));
    }
}

/// Chunks loaded by anything. Chunks not loaded by a player should not be rendered to avoid seeing
/// where npcs are
#[derive(Resource, Default)]
struct LoadedChunks(HashSet<ChunkPos>);

#[derive(Resource, Debug, Clone, Default)]
struct GeneratedChunks {
    chunks: HashMap<ChunkPos, [[TileKind; 32]; 32]>,
}

fn generate_chunk(world_seed: &[u8; 32], chunk_pos: ChunkPos) -> [[TileKind; 32]; 32] {
    let mut chunk_seed = *world_seed;
    chunk_seed[24..28].copy_from_slice(&chunk_pos.x.to_le_bytes());
    chunk_seed[28..32].copy_from_slice(&chunk_pos.y.to_le_bytes());
    let mut rng = SmallRng::from_seed(chunk_seed);
    let weights = [(TileKind::Empty, 200.0), (TileKind::Village, 5.0)];
    let dist = WeightedIndex::new(weights.iter().map(|item| item.1))
        .unwrap()
        .map(|i| weights[i].0);
    std::array::from_fn(|_| std::array::from_fn(|_| dist.sample(&mut rng)))
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
    global_from_chunk_and_local(pos, TilePos { x: 0, y: 0 }).center_in_world(&TILEMAP_GRID_SIZE)
}

pub fn chunk_center_position(pos: ChunkPos) -> Vec2 {
    let origin_pos = chunk_in_world_position(pos);
    origin_pos
        + get_tilemap_center_transform(&TILEMAP_CHUNK_SIZE, &TILEMAP_GRID_SIZE, &TILEMAP_TYPE, 0.0)
            .translation
            .truncate()
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
    chunk_data: &[[TileKind; 32]; 32],
    map_entity: Entity,
) {
    commands
        .entity(map_entity)
        .with_children(|map_child_builder| {
            let mut tile_storage = TileStorage::empty(TILEMAP_CHUNK_SIZE);
            map_child_builder
                .spawn_empty()
                .with_children(|cb| {
                    let tilemap_id = TilemapId(cb.parent_entity());

                    for x in 0..TILEMAP_CHUNK_SIZE.x {
                        for y in 0..TILEMAP_CHUNK_SIZE.y {
                            let pos = TilePos { x, y };
                            let tile_entity = cb
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
                                    TileVisibility::Unknown,
                                    chunk_data[x as usize][y as usize],
                                ))
                                .id();
                            tile_storage.set(&pos, tile_entity);
                        }
                    }
                })
                .insert((
                    TilemapBundle {
                        grid_size: TILEMAP_GRID_SIZE,
                        size: TILEMAP_CHUNK_SIZE,
                        storage: tile_storage,
                        texture: TilemapTexture::Single(texture_handle.clone()),
                        tile_size: TILEMAP_TILE_SIZE,
                        map_type: TILEMAP_TYPE,
                        transform: Transform::from_translation(
                            chunk_in_world_position(pos).extend(0.0),
                        ),
                        ..default()
                    },
                    Chunk { pos },
                ));
        });
}

fn load_chunks_player(
    mut commands: Commands,
    player_vehicles: Query<&MapPos, With<PlayerVehicle>>,
    mut loaded_chunks: ResMut<LoadedChunks>,
    map_tile_texture: Res<SpriteAssets>,
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
                    spawn_chunk(
                        &mut commands,
                        &map_tile_texture.map_tiles,
                        chunk_pos,
                        chunk_data,
                        map_entity,
                    );
                    loaded_chunks.0.insert(chunk_pos);
                }
            }
        }
    }
}

fn load_chunks_npc(
    mut commands: Commands,
    npcs: Query<&MapPos, With<Npc>>,
    mut loaded_chunks: ResMut<LoadedChunks>,
    map_tile_texture: Res<SpriteAssets>,
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
                    spawn_chunk(
                        &mut commands,
                        &map_tile_texture.map_tiles,
                        chunk_pos,
                        chunk_data,
                        map_entity,
                    );
                    loaded_chunks.0.insert(chunk_pos);
                }
            }
        }
    }
}

fn chunk_unload(
    mut commands: Commands,
    player_vehicles: Query<&MapPos, With<PlayerVehicle>>,
    npcs: Query<&MapPos, With<Npc>>,
    chunks: Query<(Entity, &Chunk)>,
    mut loaded_chunks: ResMut<LoadedChunks>,
) {
    for (chunk_entity, Chunk { pos: chunk_pos }) in chunks.iter() {
        let mut player_chunk_positions = player_vehicles
            .iter()
            .map(|mp| chunk_and_local_from_global(mp.pos).0);
        let mut npcs_chunk_positions = npcs.iter().map(|mp| chunk_and_local_from_global(mp.pos).0);
        if !(player_chunk_positions
            .any(|p| is_chunk_in_radius(p, *chunk_pos, PLAYER_CHUNK_UNLOAD_DISTANCE))
            || npcs_chunk_positions
                .any(|p| is_chunk_in_radius(p, *chunk_pos, NPC_CHUNK_UNLOAD_DISTANCE)))
        {
            commands.entity(chunk_entity).despawn_recursive();
            loaded_chunks.0.remove(chunk_pos);
        }
    }
}
