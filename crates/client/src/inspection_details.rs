//! Read current inspection details from replicated state and editable terrain.
use crate::{GameWorld, Session, inspection::InspectTarget};
use rubblekin_core::{
    protocol::{ResidentAction, ResidentRole},
    settlement::{FieldPlot, Village},
    village_assets::BuildingKind,
    world::{Block, BlockPos, CELL_SIZE, TreeKind},
};

pub(crate) fn text(world: &GameWorld, session: &Session) -> String {
    match session.inspected {
        None => "Nothing under the center dot.\n\nAim at a character, block, or farm plot, then reopen the inspector.".into(),
        Some(InspectTarget::Npc) => npc_text(session),
        Some(InspectTarget::Resident(id)) => resident_text(world, session, id),
        Some(InspectTarget::Wildlife(id)) => wildlife_text(session, id),
        Some(InspectTarget::WildForage { habitat, ground }) => forage_text(world,session,habitat,ground),
        Some(InspectTarget::Player(id)) => player_text(session, id),
        Some(InspectTarget::Block(position)) => block_text(world, session, position),
        Some(InspectTarget::FarmPlot { village, field }) => {
            farm_text(world, session, village, field)
        }
    }
}

fn forage_text(world: &GameWorld, session: &Session, id: u32, ground: BlockPos) -> String {
    let Some(h) = session.habitats.iter().find(|h| h.id == id) else {
        return "This wild habitat is no longer present.\n\nReopen the inspector to select a new target.".into();
    };
    let Some(plant) = rubblekin_core::forage::plants(world, h.id, h.position, h.forage)
        .into_iter()
        .find(|p| p.ground == ground)
    else {
        return format!(
            "These wild plants were grazed, gathered or covered.\nWild forage: {:.0}%\n\nFind another clump or let this habitat regrow.",
            h.forage
        );
    };
    let name = plant.kind.name();
    format!(
        "{name}\nWild forage: {:.0}%\n\nHome range: {} rabbits · {} wolves\n\nWild plants feed rabbits here. Grazing and gathering leave fewer plants until they regrow.\n\nMove close and open B: cargo & work to gather Food.",
        h.forage, h.rabbits, h.wolves
    )
}

fn npc_text(session: &Session) -> String {
    let npc = &session.npc;
    let mut value = format!(
        "{} · Forager\n{}{}\nBerries gathered   {}\n\n{}\n\n{}",
        npc.name,
        npc.action.label(),
        if npc.forced { " [override]" } else { "" },
        npc.berries,
        needs_text(npc.hunger, npc.energy),
        npc.reason,
    );
    if session.can_admin && session.observer.is_none() {
        value.push_str(
            "\n\nF6 forage · F7 rest\nF8 autonomous · F9 set needs\n[ favor rest · ] reset weights",
        );
    }
    value
}

fn resident_text(world: &GameWorld, session: &Session, id: u64) -> String {
    let Some(resident) = session.residents.iter().find(|resident| resident.id == id) else {
        return "This resident is no longer present.\n\nReopen the inspector to select a new target.".into();
    };
    let village = village(world, resident.village_id)
        .map_or("Unknown village", |village| village.name.as_str());
    let cargo = resident.carrying.as_ref().map_or_else(
        || "Carrying nothing".into(),
        |cargo| format!("Carrying {:.0} {}", cargo.amount, cargo.kind.name()),
    );
    format!(
        "{} · {}\n{}\n{}\n\n{}\n{}\n\n{}",
        resident.name,
        resident.role.label(),
        resident_activity(resident.action, resident.role),
        cargo,
        village,
        resident.reason,
        needs_text(resident.hunger, resident.energy),
    )
}

fn resident_activity(action: ResidentAction, role: ResidentRole) -> &'static str {
    match action {
        // Walking also covers boarding, leaving an airship, and returning to an
        // interrupted job. The authoritative reason below supplies that detail.
        ResidentAction::Walking => "Walking to the next stop",
        ResidentAction::Working => match role {
            ResidentRole::Farmer => "Working in the field",
            ResidentRole::Woodcutter => "Gathering timber",
            ResidentRole::Quarrier => "Quarrying resources",
            ResidentRole::Miner => "Mining resources",
            ResidentRole::Trader => "Working",
        },
        _ => action.label(),
    }
}

fn needs_text(hunger: f32, energy: f32) -> String {
    // These are descriptive display bands, not additional simulation states.
    // Village residents seek food at hunger >= 60 and rest at energy <= 30;
    // hunger grows as food is needed, while energy falls as rest is needed.
    let hunger_label = if hunger >= 85. {
        "Very hungry"
    } else if hunger >= 60. {
        "Hungry"
    } else if hunger >= 30. {
        "Getting hungry"
    } else {
        "Well fed"
    };
    let energy_label = if energy <= 10. {
        "Exhausted"
    } else if energy <= 30. {
        "Tired"
    } else if energy < 85. {
        "Somewhat tired"
    } else {
        "Rested"
    };
    format!("{hunger_label} · {energy_label}\nHunger {hunger:.0}/100 · Energy {energy:.0}/100")
}

fn player_text(session: &Session, id: u64) -> String {
    let Some(player) = session.players.iter().find(|player| player.id == id) else {
        return "This explorer has left the world.\n\nReopen the inspector to select a new target."
            .into();
    };
    let [x, y, z] = player.body.position;
    format!(
        "{} · Explorer\n\nPosition (m)\n{:.1}, {:.1}, {:.1}",
        player.name, x, y, z,
    )
}

fn block_text(world: &GameWorld, session: &Session, position: BlockPos) -> String {
    let block = world.block(position);
    let details = format!(
        "{} block\n\nCell {}, {}, {}\n0.5 m × 0.5 m × 0.5 m",
        block.name(),
        position.x,
        position.y,
        position.z,
    );
    if block == Block::Air {
        return format!("{details}\n\nThe inspected block was removed.");
    }
    let mut value = details.clone();
    if matches!(block, Block::Wood | Block::Leaves)
        && let Some(tree) = world.tree_at(position.x.div_euclid(24), position.z.div_euclid(24))
    {
        let dx = position.x - tree.base.x;
        let dz = position.z - tree.base.z;
        let trunk = block == Block::Wood
            && dx == 0
            && dz == 0
            && (tree.base.y..tree.base.y + tree.trunk_height).contains(&position.y);
        let crown = block == Block::Leaves
            && tree
                .leaf_bounds(dx, dz)
                .is_some_and(|(low, high)| (low..=high).contains(&position.y));
        if trunk || crown {
            value = format!("{}\n\n{}", tree_name(tree.kind), details);
        }
    }
    if let Some(site) = world.settlements().and_then(|plan| {
        plan.roadside_landmarks.iter().find(|site| {
            let building = &site.building;
            let [_, height, _] = building.dimensions();
            building.local_cell(position.x, position.z).is_some()
                && (building.origin.y..building.origin.y + height).contains(&position.y)
        })
    }) {
        return format!(
            "{}\n{}\n\n{}\n\n{}",
            building_name(site.building.kind),
            if site.approach.is_some() {
                "Beside the village trail"
            } else {
                "In the wilderness, away from trails"
            },
            building_description(site.building.kind),
            details,
        );
    }
    if let Some((village, building)) = world.settlements().and_then(|plan| {
        plan.villages.iter().find_map(|village| {
            village.buildings.iter().find_map(|building| {
                let [_, height, _] = building.dimensions();
                (building.local_cell(position.x, position.z).is_some()
                    && (building.origin.y..building.origin.y + height).contains(&position.y))
                .then_some((village, building))
            })
        })
    }) {
        value = format!(
            "{}\n{} · {}\n\n{}\n\n{}",
            building_name(building.kind),
            village.name,
            village.kind.name(),
            building_description(building.kind),
            details
        );
        if building.kind == BuildingKind::Market && session.observer.is_none() {
            value.push_str("\n\nStand at the entrance, then press B or tap Cargo to trade or take delivery work.");
        }
        if building.kind == BuildingKind::Workshop && session.observer.is_none() {
            value.push_str("\n\nWalk inside to the stone workbench, then press B or tap Cargo to help with workshop maintenance.");
        }
        if matches!(
            building.kind,
            BuildingKind::Storehouse | BuildingKind::Market
        ) && let Some(stores) = session.villages.iter().find(|v| v.id == village.id)
        {
            value.push_str(&format!(
                "\n\nVillage stores\nFood {:.0} · Timber {:.0}\nStone {:.0} · Clay {:.0}\nIron {:.0}",
                stores.food, stores.timber, stores.stone, stores.clay, stores.iron,
            ));
        }
    }
    value
}

fn tree_name(kind: TreeKind) -> &'static str {
    match kind {
        TreeKind::Broadleaf => "Broadleaf tree",
        TreeKind::Conifer => "Pine tree",
        TreeKind::Scrub => "Scrub bush",
        TreeKind::Aspen => "Aspen · narrow crown",
        TreeKind::Cedar => "Cedar · layered crown",
        TreeKind::Canopy => "Rainforest tree · spreading canopy",
    }
}

fn building_name(kind: BuildingKind) -> &'static str {
    match kind {
        BuildingKind::Cottage => "Cottage",
        BuildingKind::Workshop => "Workshop",
        BuildingKind::Storehouse => "Storehouse",
        BuildingKind::Market => "Market",
        BuildingKind::TimberCabin => "Timber cabin",
        BuildingKind::MasonryCottage => "Masonry cottage",
        BuildingKind::UplandHouse => "Upland house",
        BuildingKind::Windmill => "Windmill",
        BuildingKind::Lookout => "Lookout",
        BuildingKind::TrailRuin => "Trail ruin",
        BuildingKind::Waystone => "Waystone",
        BuildingKind::TrailPavilion => "Trail shelter",
        BuildingKind::QuarryYard => "Quarry workyard",
        BuildingKind::StoneArch => "Stone arch",
        BuildingKind::StandingStones => "Standing stones",
        BuildingKind::FallenGiant => "Fallen giant",
        BuildingKind::TrailCamp => "Traveller camp",
        BuildingKind::RuinedTower => "Ruined watchtower",
        BuildingKind::AbandonedKiln => "Abandoned kiln",
        BuildingKind::RidgeCairn => "Ridge cairns",
        BuildingKind::TrailBench => "Trail bench",
        BuildingKind::CartWreck => "Broken waycart",
        BuildingKind::SurveyPost => "Survey post",
        BuildingKind::DeadSnag => "Dead snag",
        BuildingKind::SplitBoulder => "Split boulder",
        BuildingKind::CliffDeck => "Cliff viewing deck",
    }
}

fn building_description(kind: BuildingKind) -> &'static str {
    match kind {
        BuildingKind::Cottage => "A compact furnished home with a hearth and a brick roof.",
        BuildingKind::Workshop => "Covered workbenches and a stone work surface serve the village.",
        BuildingKind::Storehouse => {
            "Village storage, with timber bins beside an open central aisle."
        }
        BuildingKind::Market => "Open stalls for village trade and paid deliveries.",
        BuildingKind::TimberCabin => "Timber walls and a low wooden roof frame a furnished home.",
        BuildingKind::MasonryCottage => {
            "Stone corners and a low hipped roof frame a furnished home."
        }
        BuildingKind::UplandHouse => {
            "A steep brick roof and high gable windows distinguish this home."
        }
        BuildingKind::Windmill => {
            "A tall village landmark with four fixed sails and a ground-floor room."
        }
        BuildingKind::Lookout => "Stairs lead to a raised, covered viewing deck.",
        BuildingKind::TrailRuin => {
            "Weathered stone walls enclose an open courtyard. A short spur returns to the trail."
        }
        BuildingKind::Waystone => {
            "A banded stone marker stands beside the trail, with room to stop and look around."
        }
        BuildingKind::TrailPavilion => {
            "An open timber roof shelters a resting place beside the trail."
        }
        BuildingKind::QuarryYard => {
            "A stone cutting terrace and workyard stand near a natural stone deposit."
        }
        BuildingKind::StoneArch => {
            "An uneven natural rock span frames the landscape. Walk beneath it to see the other side."
        }
        BuildingKind::StandingStones => {
            "Unequal uprights and fallen fragments form a broken ring with an open center."
        }
        BuildingKind::FallenGiant => {
            "A hollow old trunk lies among exposed roots and surviving branches."
        }
        BuildingKind::TrailCamp => {
            "Two canvas tents, benches and a cold hearth mark a quiet traveller rest stop."
        }
        BuildingKind::RuinedTower => {
            "A roofless watchtower has a surviving stair to a low viewing ledge."
        }
        BuildingKind::AbandonedKiln => {
            "A cold brick firing chamber and drying racks stand near natural clay. Use Cargo & work beside the old clay stacks to salvage their finite supplies."
        }
        BuildingKind::RidgeCairn => "Small hand-stacked stones mark an old stopping place.",
        BuildingKind::TrailBench => {
            "A rough timber bench and slatted windbreak offer a quiet view."
        }
        BuildingKind::CartWreck => {
            "A broken waycart has lost a wheel and both its cargo and travellers. Use Cargo & work beside loose timber or wheel hubs to salvage finite supplies."
        }
        BuildingKind::SurveyPost => {
            "A survey tripod, sighting stakes and folded tarp overlook the route."
        }
        BuildingKind::DeadSnag => {
            "A weathered tree skeleton spreads bare branches above the surrounding ground."
        }
        BuildingKind::SplitBoulder => {
            "An eroded rock has split into two halves with a narrow passage."
        }
        BuildingKind::CliffDeck => {
            "A small timber deck on piles overlooks the hillside beside the raised trail."
        }
    }
}

fn farm_text(world: &GameWorld, session: &Session, id: u32, index: usize) -> String {
    let Some(village) = village(world, id) else {
        return "This village is no longer available.\n\nReopen the inspector to select a new target.".into();
    };
    let Some(field) = village.fields.get(index) else {
        return "This farm plot is no longer available.\n\nReopen the inspector to select a new target.".into();
    };
    let (intact, total) = plant_sites(world, field);
    let mut value = format!(
        "Farm plot {}\n{}\n\nSize {:.1} × {:.1} m\nIntact plant sites   {} / {}",
        index + 1,
        village.name,
        field.width as f32 * CELL_SIZE,
        field.depth as f32 * CELL_SIZE,
        intact,
        total,
    );
    value.push_str(&format!(
        "\nCrop appearance   {}",
        crate::crops::crop_kind(world, village, index).name()
    ));
    if let Some(state) = session.villages.iter().find(|v| v.id == id) {
        let readiness = if intact == 0 {
            "No crops on this plot"
        } else if state.crop_growth >= 1.0 {
            "Ready to harvest"
        } else if state.crop_growth > 0.0 {
            "Growing"
        } else {
            "Awaiting planting"
        };
        value.push_str(&format!(
            "\n\nCrop growth   {:.0}%\n{}\nShared village crop cycle\n\nVillage food store   {:.0}",
            state.crop_growth * 100.0,
            readiness,
            state.food,
        ));
    } else {
        value.push_str("\n\nWaiting for village growth data.");
    }
    if intact == 0 {
        value.push_str("\n\nThis plot has no intact plant soil.");
    } else if intact < total {
        value.push_str("\n\nSome plant soil was removed or replaced.");
    }
    if intact > 0 && session.observer.is_none() {
        value.push_str("\n\nStand beside the field and open B or Cargo to tend crops or harvest surplus food for market sale.");
    }
    value
}

fn village(world: &GameWorld, id: u32) -> Option<&Village> {
    world
        .settlements()?
        .villages
        .iter()
        .find(|village| village.id == id)
}

fn plant_sites(world: &GameWorld, field: &FieldPlot) -> (usize, usize) {
    field
        .plant_positions()
        .fold((0, 0), |(intact, total), position| {
            (
                intact + usize::from(matches!(world.block(position), Block::Dirt | Block::Grass)),
                total + 1,
            )
        })
}

fn wildlife_text(session: &Session, id: u64) -> String {
    let Some(a) = session.wildlife.iter().find(|a| a.id == id) else {
        return "This animal has moved away or is no longer alive.\n\nReopen the inspector to select a new target.".into();
    };
    let habitat = session
        .habitats
        .iter()
        .find(|h| h.id == a.habitat)
        .map_or(String::new(), |h| {
            format!(
                "\n\nHome range: {} rabbits · {} wolves\nWild forage: {:.0}%",
                h.rabbits, h.wolves, h.forage
            )
        });
    format!(
        "{}\n{}\n\nHunger: {:.0}%{}\n\nWildlife keeps its distance from people. Rabbits graze wild plants; wolves hunt rabbits. Populations breed and move as food changes.",
        a.species.name(),
        a.action.label(),
        a.hunger,
        habitat
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{graphics::GraphicsQuality, join::session_from_welcome};
    use rubblekin_core::{
        protocol::{
            NpcAction, NpcSnapshot, PROTOCOL_VERSION, ServerMessage, SessionMode, VillageSnapshot,
        },
        world::WorldGeneration,
    };

    fn session() -> (GameWorld, Session) {
        session_from_welcome(
            ServerMessage::Welcome {
                version: PROTOCOL_VERSION,
                session_id: 1,
                mode: SessionMode::Observer,
                seed: 42,
                generation: WorldGeneration::GeographyV3,
                edits: vec![],
                players: vec![],
                npc: NpcSnapshot {
                    name: "Moss".into(),
                    position: [0.; 3],
                    hunger: 25.,
                    energy: 85.,
                    action: NpcAction::Forage,
                    reason: "Looking for berries".into(),
                    berries: 0,
                    forced: false,
                    target: None,
                },
                residents: vec![],
                villages: vec![],
                world_time: 0.,
                can_admin: false,
            },
            "test".into(),
            GraphicsQuality::default(),
            0.,
            SessionMode::Observer,
        )
        .unwrap()
    }

    #[test]
    fn roadside_places_and_regional_trees_describe_actual_cells_and_removed_targets() {
        let (_, mut session) = session();
        let mut world = GameWorld::generate(42, WorldGeneration::GeographyV6);
        let buildings: Vec<_> = world
            .settlements()
            .unwrap()
            .roadside_landmarks
            .iter()
            .map(|site| (site.building.clone(), site.approach.is_some()))
            .collect();
        assert!(
            buildings
                .iter()
                .any(|(b, _)| b.kind == BuildingKind::TrailRuin)
        );
        assert!(
            buildings
                .iter()
                .any(|(b, _)| b.kind == BuildingKind::Waystone)
        );
        assert!(buildings.iter().any(|(b, _)| matches!(
            b.kind,
            BuildingKind::TrailPavilion | BuildingKind::QuarryYard
        )));
        for (building, beside_trail) in &buildings {
            session.inspected = Some(InspectTarget::Block(building.origin));
            let details = text(&world, &session);
            assert!(
                details.starts_with(building_name(building.kind)),
                "{details}"
            );
            assert_eq!(details.contains("Beside the village trail"), *beside_trail);
            assert_eq!(
                details.contains("In the wilderness, away from trails"),
                !beside_trail
            );
            world.set_block(building.origin, Block::Air).unwrap();
            let removed = text(&world, &session);
            assert!(removed.contains("was removed"));
            assert!(!removed.contains("Beside the village trail"));
        }
        let tree = (-1300..1300)
            .step_by(7)
            .find_map(|z| {
                (-1300..1300).step_by(7).find_map(|x| {
                    world.tree_at(x, z).filter(|tree| {
                        matches!(
                            tree.kind,
                            TreeKind::Aspen | TreeKind::Cedar | TreeKind::Canopy
                        )
                    })
                })
            })
            .expect("seed 42 has regional woodland");
        let trunk = BlockPos::new(tree.base.x, tree.base.y + 1, tree.base.z);
        assert_eq!(world.block(trunk), Block::Wood);
        session.inspected = Some(InspectTarget::Block(trunk));
        assert!(text(&world, &session).starts_with(tree_name(tree.kind)));
        world.set_block(trunk, Block::Stone).unwrap();
        assert!(!text(&world, &session).contains(tree_name(tree.kind)));
    }

    #[test]
    fn regional_building_inspection_leads_with_the_actual_place_and_preserves_selected_cell() {
        let (_, mut session) = session();
        let mut world = GameWorld::generate(42, WorldGeneration::GeographyV4);
        let sites: Vec<_> = world
            .settlements()
            .unwrap()
            .villages
            .iter()
            .flat_map(|village| {
                village
                    .buildings
                    .iter()
                    .map(|building| (village.name.clone(), building.kind, building.origin))
            })
            .collect();
        let mut landmarks = 0;
        let mut regional_homes = 0;
        for (name, kind, position) in &sites {
            if !matches!(
                kind,
                BuildingKind::TimberCabin
                    | BuildingKind::MasonryCottage
                    | BuildingKind::UplandHouse
                    | BuildingKind::Windmill
                    | BuildingKind::Lookout
            ) {
                continue;
            }
            session.inspected = Some(InspectTarget::Block(*position));
            let details = text(&world, &session);
            assert!(
                details.starts_with(&format!("{}\n{}", building_name(*kind), name)),
                "{details}"
            );
            assert!(details.contains(&format!(
                "Cell {}, {}, {}",
                position.x, position.y, position.z
            )));
            assert!(details.contains(&format!("{} block", world.block(*position).name())));
            assert!(!details.contains("Nothing under"));
            if kind.is_landmark() {
                landmarks += 1;
            } else {
                regional_homes += 1;
            }
        }
        assert!(landmarks > 0 && regional_homes > 0);
        let position = sites
            .iter()
            .find(|(_, kind, _)| kind.is_landmark())
            .unwrap()
            .2;
        session.inspected = Some(InspectTarget::Block(position));
        world.set_block(position, Block::Air).unwrap();
        let removed = text(&world, &session);
        assert!(removed.contains("inspected block was removed"));
        assert!(!removed.contains("viewing deck") && !removed.contains("four fixed sails"));
    }

    #[test]
    fn market_inspection_keeps_live_stores_and_only_offers_controls_to_a_player() {
        let (world, mut session) = session();
        let village = &world.settlements().unwrap().villages[0];
        let market = village
            .buildings
            .iter()
            .find(|building| building.kind == BuildingKind::Market)
            .unwrap();
        session.inspected = Some(InspectTarget::Block(market.origin));
        session.villages.push(VillageSnapshot {
            id: village.id,
            food: 13.,
            timber: 2.,
            stone: 3.,
            clay: 4.,
            iron: 5.,
            crop_growth: 0.,
            population: 6,
            housing_capacity: 6,
            food_reserve: 10.,
            capacity_for_growth: false,
        });
        let observer = text(&world, &session);
        assert!(observer.starts_with("Market\n"));
        assert!(observer.contains("Food 13"));
        assert!(!observer.contains("press B"));
        session.observer = None;
        session.villages[0].food = 21.;
        let player = text(&world, &session);
        assert!(player.contains("Food 21") && !player.contains("Food 13"));
        assert!(
            player.contains("Stand at the entrance") && player.contains("press B or tap Cargo")
        );
    }

    #[test]
    fn farm_details_follow_live_growth_and_edited_plant_soil() {
        let (mut world, mut session) = session();
        let village = &world.settlements().unwrap().villages[0];
        let id = village.id;
        let field = village.fields[0].clone();
        let total = field.plant_positions().count();
        session.inspected = Some(InspectTarget::FarmPlot {
            village: id,
            field: 0,
        });
        session.villages.push(VillageSnapshot {
            id,
            food: 80.,
            timber: 0.,
            stone: 0.,
            clay: 0.,
            iron: 0.,
            crop_growth: 0.45,
            population: 6,
            housing_capacity: 9,
            food_reserve: 20.,
            capacity_for_growth: false,
        });
        let growing = text(&world, &session);
        assert!(growing.contains("Crop growth   45%"));
        assert!(growing.contains("Shared village crop cycle"));
        assert!(growing.contains(&format!("Intact plant sites   {total} / {total}")));
        session.villages[0].crop_growth = 1.;
        assert!(text(&world, &session).contains("Ready to harvest"));
        for position in field.plant_positions() {
            world.set_block(position, Block::Brick).unwrap();
        }
        let removed = text(&world, &session);
        assert!(removed.contains(&format!("Intact plant sites   0 / {total}")));
        assert!(removed.contains("No crops on this plot"));
        assert!(!removed.contains("Ready to harvest"));
        world
            .set_block(field.plant_positions().next().unwrap(), Block::Grass)
            .unwrap();
        assert!(text(&world, &session).contains(&format!("Intact plant sites   1 / {total}")));
    }

    #[test]
    fn removed_and_missing_targets_are_reported_without_falling_back_to_nearby_npc() {
        let (mut world, mut session) = session();
        let position = world.settlements().unwrap().villages[0].fields[0].origin;
        session.inspected = Some(InspectTarget::Block(position));
        world.set_block(position, Block::Air).unwrap();
        assert!(text(&world, &session).contains("inspected block was removed"));
        session.inspected = Some(InspectTarget::Resident(u64::MAX));
        assert!(text(&world, &session).contains("no longer present"));
        session.inspected = None;
        assert!(text(&world, &session).contains("Nothing under the center dot"));
        session.inspected = Some(InspectTarget::Npc);
        session.can_admin = true;
        assert!(!text(&world, &session).contains("F6"));
    }

    #[test]
    fn needs_words_explain_opposite_scales_and_preserve_numeric_details() {
        for (hunger, energy, expected) in [
            (0., 100., "Well fed · Rested"),
            (29., 85., "Well fed · Rested"),
            (30., 84., "Getting hungry · Somewhat tired"),
            (59., 31., "Getting hungry · Somewhat tired"),
            (60., 30., "Hungry · Tired"),
            (84., 11., "Hungry · Tired"),
            (85., 10., "Very hungry · Exhausted"),
            (100., 0., "Very hungry · Exhausted"),
        ] {
            let text = needs_text(hunger, energy);
            assert!(text.starts_with(expected), "{text}");
            assert!(text.contains(&format!("Hunger {hunger:.0}/100 · Energy {energy:.0}/100")));
        }
    }

    #[test]
    fn fixed_resident_inspection_leads_with_activity_cargo_and_live_authoritative_reason() {
        use rubblekin_core::{
            protocol::{ResidentSnapshot, ResourceCargo},
            settlement::ResourceKind,
        };
        let (world, mut session) = session();
        let home = &world.settlements().unwrap().villages[0];
        session.residents.push(ResidentSnapshot {
            id: 7,
            village_id: home.id,
            name: "Juniper".into(),
            position: [0.; 3],
            role: ResidentRole::Farmer,
            action: ResidentAction::Harvesting,
            target: None,
            carrying: Some(ResourceCargo {
                kind: ResourceKind::Food,
                amount: 4.,
            }),
            hunger: 75.,
            energy: 20.,
            reason: "Gathering ripe crops before carrying them to storage".into(),
            ride: None,
            deck_position: None,
        });
        session.inspected = Some(InspectTarget::Resident(7));
        let details = text(&world, &session);
        assert!(
            details.starts_with("Juniper · Farmer\nHarvesting crops\nCarrying 4 Food"),
            "{details}"
        );
        assert!(details.contains(&home.name));
        assert!(details.contains(&session.residents[0].reason));
        assert!(details.contains("Hungry · Tired\nHunger 75/100 · Energy 20/100"));
        {
            let resident = &mut session.residents[0];
            resident.action = ResidentAction::Walking;
            resident.reason = "Arrived by airship; walking to the trade destination".into();
            resident.carrying = None;
            resident.hunger = 25.;
            resident.energy = 90.;
        }
        let updated = text(&world, &session);
        assert!(updated.contains("Walking to the next stop\nCarrying nothing"));
        assert!(!updated.contains("Walking to work"));
        assert!(updated.contains(&session.residents[0].reason));
        assert!(updated.contains("Well fed · Rested\nHunger 25/100 · Energy 90/100"));
        assert_eq!(session.inspected, Some(InspectTarget::Resident(7)));
    }

    #[test]
    fn moss_keeps_decision_scores_override_and_permission_gated_admin_controls() {
        let (world, mut session) = session();
        session.inspected = Some(InspectTarget::Npc);
        session.npc.reason = "Forage score 60, rest score 25, wander threshold 25; Foraging".into();
        session.npc.berries = 12;
        let normal = text(&world, &session);
        assert!(
            normal.starts_with("Moss · Forager\nForaging\nBerries gathered   12"),
            "{normal}"
        );
        assert!(normal.contains(&session.npc.reason));
        assert!(!normal.contains("F6"));
        session.can_admin = true;
        assert!(
            !text(&world, &session).contains("F6"),
            "observer stays read-only"
        );
        session.observer = None;
        session.npc.forced = true;
        session.npc.reason = "Admin override: Foraging (clear override to restore autonomy)".into();
        let overridden = text(&world, &session);
        assert!(overridden.contains("Foraging [override]"));
        assert!(overridden.contains(&session.npc.reason));
        assert!(overridden.contains("F6 forage · F7 rest"));
        assert!(overridden.contains("F8 autonomous"));
    }
}
