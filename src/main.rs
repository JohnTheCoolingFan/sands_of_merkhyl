#![allow(clippy::type_complexity)]

use bevy::{
    input::mouse::{MouseScrollUnit, MouseWheel},
    prelude::*,
    render::camera::ScalingMode,
    sprite::Anchor,
    utils::HashSet,
    window::PresentMode,
};
use bevy_ecs_tilemap::prelude::*;
use bevy_prototype_lyon::prelude::*;

const ASPECT_RATIO: f32 = 16.0 / 9.0;
const CLEAR_COLOR: Color = Color::rgb(0.1, 0.1, 0.1);
const VISIBLE_TILE_COLOR: TileColor = TileColor(Color::rgb(1.0, 1.0, 1.0));
const CHARTED_TILE_COLOR: TileColor = TileColor(Color::rgb(0.3, 0.3, 0.3));
const MAP_VIEW_SCALE: f32 = 1.0;
const PLATFORM_VIEW_SCALE: f32 = 25.0;
const TILEMAP_CHUNK_SIZE: TilemapSize = TilemapSize { x: 16, y: 16 };
const MAP_TILEMAP_Z: f32 = 900.0;

type ChunkPos = IVec2;

#[derive(Component, Debug, Clone, Copy)]
enum TileVisibility {
    Visible,
    Charted,
    Unknown,
}

#[derive(Component, Debug, Clone, Copy)]
#[repr(u8)]
enum TileKind {
    Empty = 1,
}

#[derive(Component)]
struct Chunk {
    pos: ChunkPos,
}

#[derive(Resource)]
struct RenderedChunks(HashSet<ChunkPos>);

#[derive(Resource)]
struct MiningPlatformSprite(Handle<Image>);

#[derive(Resource)]
struct TileSprite(Handle<Image>);

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

#[derive(Component)]
struct Map;

fn components_from_global(global_pos: IVec2) -> (ChunkPos, TilePos) {
    let chunk_pos = ChunkPos::new(global_pos.x.div_euclid(TILEMAP_CHUNK_SIZE.x as i32), global_pos.y.div_euclid(TILEMAP_CHUNK_SIZE.y as i32));
    let tile_pos = TilePos { x: global_pos.x.rem_euclid(TILEMAP_CHUNK_SIZE.x as i32) as u32, y: global_pos.y.rem_euclid(TILEMAP_CHUNK_SIZE.y as i32) as u32 };
    (chunk_pos, tile_pos)
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
    texture_handle: Res<TileSprite>,
    mut rendered_chunks: ResMut<RenderedChunks>,
) {
    let mut chunks = Vec::new();

    for x in 0..3 {
        for y in 0..3 {
            let pos = IVec2::new(x, y);
            chunks.push(spawn_visual_chunk(&mut commands, &texture_handle.0, pos));
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

fn spawn_visual_chunk(commands: &mut Commands, texture_handle: &Handle<Image>, pos: ChunkPos) -> Entity {
    let mut tile_storage = TileStorage::empty(TILEMAP_CHUNK_SIZE);
    let tilemap_entity = commands.spawn_empty().id();
    let tilemap_id = TilemapId(tilemap_entity);

    fill_tilemap_rect(
        TileTextureIndex(0),
        TilePos { x: 0, y: 0 },
        TILEMAP_CHUNK_SIZE,
        tilemap_id,
        commands,
        &mut tile_storage,
    );

    let tile_size = TilemapTileSize { x: 28.0, y: 32.0 };
    let grid_size = tile_size.into();

    commands.entity(tilemap_entity).insert((
        TilemapBundle {
            grid_size,
            size: TILEMAP_CHUNK_SIZE,
            storage: tile_storage,
            texture: TilemapTexture::Single(texture_handle.clone()),
            tile_size,
            map_type: TilemapType::Hexagon(HexCoordSystem::RowEven),
            transform: Transform::from_xyz(
                tile_size.x * TILEMAP_CHUNK_SIZE.x as f32 * pos.x as f32,
                TilePos {
                    x: 0,
                    y: TILEMAP_CHUNK_SIZE.y,
                }
                .center_in_world(&grid_size, &TilemapType::Hexagon(HexCoordSystem::RowEven))
                .y * pos.y as f32,
                0.0,
            ),
            ..default()
        },
        Chunk { pos },
    ));

    tilemap_entity
}

fn update_texture(tiles: Query<(&mut TileTextureIndex, &mut TileColor, &TileVisibility, &TileKind), Or<(Changed<TileVisibility>, Changed<TileKind>)>>) {
    todo!()
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
    let tile_texture = assets.load("map_tile.png");
    commands.insert_resource(TileSprite(tile_texture));
}

fn main() {
    let height = 900.0;
    App::new()
        .insert_resource(ClearColor(CLEAR_COLOR))
        .insert_resource(CurrentView::Platform)
        .insert_resource(RenderedChunks(HashSet::new()))
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
        .run();
}
