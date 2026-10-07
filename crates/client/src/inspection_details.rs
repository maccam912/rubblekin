//! Read current inspection details from replicated state and editable terrain.
use crate::{GameWorld, Session, inspection::InspectTarget};
use rubblekin_core::{
    protocol::{ResidentAction, ResidentRole},
    settlement::{FieldPlot, Village},
    village_assets::BuildingKind,
    world::{Block, BlockPos, CELL_SIZE},
};

pub(crate) fn text(world: &GameWorld, session: &Session) -> String {
    match session.inspected {
        None => "Nothing under the center dot.\n\nAim at a character, block, or farm plot, then reopen the inspector.".into(),
        Some(InspectTarget::Npc) => npc_text(session),
        Some(InspectTarget::Resident(id)) => resident_text(world, session, id),
        Some(InspectTarget::Player(id)) => player_text(session, id),
        Some(InspectTarget::Block(position)) => block_text(world, session, position),
        Some(InspectTarget::FarmPlot { village, field }) => {
            farm_text(world, session, village, field)
        }
    }
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
    let mut value = format!(
        "{} block\n\nCell {}, {}, {}\n0.5 m × 0.5 m × 0.5 m",
        block.name(),
        position.x,
        position.y,
        position.z,
    );
    if block == Block::Air {
        value.push_str("\n\nThe inspected block was removed.");
        return value;
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
        value.push_str(&format!("\n\n{:?}\n{}", building.kind, village.name));
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
