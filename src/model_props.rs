//! The level editor's library props in a level: Portal's and Portal 2's models, extracted by
//! `portal_extract` into `props/`, drawn where the level puts them (`Level::models`).

use bevy::prelude::*;

use crate::chamber::ChamberGeom;
use crate::Game;

/// Spawn the level's models when a level loads (they go with the rest of its geometry).
pub fn spawn_for_level(mut commands: Commands, game: Res<Game>, assets: Res<AssetServer>, mut loaded: Local<u64>) {
    if *loaded == game.loads {
        return;
    }
    *loaded = game.loads;
    for m in &game.sim.level.models {
        let scene = assets.load(GltfAssetLabel::Scene(0).from_asset(format!("extracted/{}", m.path)));
        let tf = Transform::from_translation(m.at).with_rotation(m.turn).with_scale(m.scale);
        commands.spawn((ChamberGeom, crate::chamber::PlacedAt(m.at), WorldAssetRoot(scene), tf, Visibility::default()));
    }
}
