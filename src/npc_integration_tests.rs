use super::*;

#[test]
fn avatar_only_scene_runs_in_both_views_and_restores_quest_checkpoint() {
    let mut studio = Studio::default();
    studio.set_scene(Scene::empty(), None, "");
    let source = assets::Source::Emma;
    studio.assets.ensure(&source, None).unwrap();
    studio.assign_avatar(source, "Эмма".into());
    let id = npcs::FIRST_NPC_ID;
    let mut npc = studio.scene.avatar.clone().unwrap();
    npc.name = "Смотритель".into();
    studio.scene.npcs.push(npcs::Npc::new(
        id,
        npc,
        vec![npcs::Waypoint::at([1.15, 0.0, 0.0])],
    ));
    studio.scene.story = story::Story::demo(id, [3.0, -2.0]);

    let ctx = egui::Context::default();
    studio.view = View::Two;
    studio.overhead = true;
    assert!(studio.scene.parts.is_empty() && studio.can_play());
    studio.start_play(&ctx, false);
    assert!(studio.game_running);
    studio.stop();
    studio.view = View::Three;
    assert!(studio.can_play());
    studio.start_play(&ctx, false);
    assert!(studio.story_session.open(&studio.scene.story, id));
    assert!(studio.story_session.choose(&studio.scene.story, 0));
    studio.story_session.close();
    studio.view = View::Two;
    assert_eq!(
        studio.story_session.active_destination(&studio.scene.story),
        Some([3.0, -2.0])
    );
    [studio.game.x, studio.game.height, studio.game.z] = [3.0, 0.0, -2.0];
    studio
        .story_session
        .update(&studio.scene.story, [3.0, -2.0]);
    studio.stop();
    let file = std::env::temp_dir().join(format!(
        "studio-checkpoint-{}.robot.json",
        std::process::id()
    ));
    studio.scene.save(&file).unwrap();
    let loaded = Scene::load(&file);
    std::fs::remove_file(&file).unwrap();
    studio.set_scene(loaded.unwrap(), None, "");
    studio.start_play(&ctx, true);
    assert_eq!(studio.game_position(), Vec3::new(3.0, 0.0, -2.0));
    assert!(
        studio
            .story_session
            .objective(&studio.scene.story)
            .unwrap()
            .starts_with("Выполнено")
    );
    studio.stop();
    studio.start_play(&ctx, false);
    assert_eq!(studio.game_position(), Vec3::ZERO);
    assert!(
        studio
            .story_session
            .objective(&studio.scene.story)
            .is_none()
    );
}

#[test]
fn saved_city_restores_the_authored_cast_routes_and_story() {
    let mut studio = Studio::default();
    studio.city_demo();
    let file = std::env::temp_dir().join(format!(
        "robot-studio-08-story-{}.robot.json",
        std::process::id()
    ));
    studio.scene.save(&file).unwrap();
    let restored = Scene::load(&file);
    std::fs::remove_file(&file).unwrap();
    let restored = restored.unwrap();
    assert_eq!(restored.npcs, studio.scene.npcs);
    assert_eq!(restored.story, studio.scene.story);
    studio.set_scene(restored, None, "");
    assert_eq!(studio.npc_draws().len(), studio.scene.npcs.len());
}

#[test]
fn city_npc_walks_through_native_doors_in_studio() {
    let mut studio = Studio::default();
    studio.city_demo();
    studio.game_running = true;
    studio.game.x = 18.0;
    studio.game.z = 18.0;

    let furnished_house = &studio.scene.world.buildings[0];
    let route_actor = studio
        .scene
        .npcs
        .iter()
        .find(|npc| {
            npc.route.iter().any(|point| {
                let [x, y, z] = point.position;
                furnished_house.contains_interior(x, y, z)
            })
        })
        .expect("the city has a route inside its furnished house");
    let actor_id = route_actor.id;
    let spawn = Vec3::from_array(route_actor.route[0].position);
    let doors = furnished_house.doors().count();
    assert!(doors >= 2);
    assert!(
        furnished_house
            .doors()
            .all(|door| studio.interactions.fraction(&door) < 0.01)
    );

    let mut walked = false;
    let mut walked_inside = false;
    let mut returned = false;
    let ctx = egui::Context::default();
    for frame in 0..640 {
        let _ = ctx.run(
            egui::RawInput {
                time: Some(f64::from(frame) * 0.05),
                predicted_dt: 0.05,
                ..Default::default()
            },
            |ctx| studio.advance(ctx),
        );
        let actor = studio
            .npc_simulation
            .state(actor_id)
            .expect("route is running");
        let [x, y, z] = actor.position();
        let house = &studio.scene.world.buildings[0];
        walked |= actor.motion == Motion::Walk;
        walked_inside |= house.contains_interior(x, y, z)
            && house
                .doors()
                .all(|door| studio.interactions.fraction(&door) > 0.99);
        returned |= walked_inside && Vec3::new(x, y, z).distance(spawn) < 0.27;
        if returned {
            break;
        }
    }
    assert!(walked, "the route selects the walking clip");
    assert!(
        walked_inside,
        "the route opens both doors and enters the house"
    );
    assert!(returned, "the actor loops back to the street");
}

#[test]
fn city_dialogue_pauses_the_speaker_and_starts_a_reachable_visit_objective() {
    let mut studio = Studio::default();
    studio.city_demo();
    studio.game_running = true;
    studio.game.x = 18.0;
    studio.game.z = 18.0;
    studio.advance_npcs(0.05);
    assert_eq!(studio.nearby_talk_npc(), None);

    let story = &studio.scene.story;
    let quest = story
        .quests
        .iter()
        .find(|quest| matches!(quest.objective, story::Objective::Visit { .. }))
        .expect("the city includes an authored destination");
    let story::Objective::Visit {
        xz: destination, ..
    } = quest.objective
    else {
        unreachable!()
    };
    let completion = quest.completion.clone();
    let guide = story
        .conversations
        .iter()
        .find(|conversation| {
            conversation.nodes.iter().any(|node| {
                node.id == conversation.entry
                    && node
                        .choices
                        .iter()
                        .any(|choice| choice.starts_quest == Some(quest.id))
            })
        })
        .expect("a guide offers the visit objective");
    let guide_id = guide.npc_id;
    let choice = guide
        .nodes
        .iter()
        .find(|node| node.id == guide.entry)
        .unwrap()
        .choices
        .iter()
        .position(|choice| choice.starts_quest == Some(quest.id))
        .unwrap();

    let guide_position = studio
        .npc_simulation
        .state(guide_id)
        .expect("guide is spawned")
        .position();
    [studio.game.x, studio.game.height, studio.game.z] = guide_position;
    assert_eq!(studio.nearby_talk_npc(), Some(guide_id));
    let ctx = egui::Context::default();
    let _ = ctx.run(
        egui::RawInput {
            events: vec![egui::Event::Key {
                key: Key::E,
                physical_key: Some(Key::E),
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            ..Default::default()
        },
        |ctx| studio.advance(ctx),
    );
    assert_eq!(studio.story_session.active_npc(), Some(guide_id));
    for _ in 0..25 {
        studio.advance_npcs(0.05);
    }
    let guide_state = studio.npc_simulation.state(guide_id).unwrap();
    let [x, _, z] = guide_state.position();
    assert_eq!([x, z], [guide_position[0], guide_position[2]]);
    assert_eq!(guide_state.motion, Motion::Idle);
    assert!(studio.story_session.choose(&studio.scene.story, choice));
    assert_eq!(
        studio.story_session.active_destination(&studio.scene.story),
        Some(destination)
    );
    studio.story_session.close();

    let [x, z] = destination;
    let floor = studio
        .scene
        .world
        .buildings
        .iter()
        .find(|house| house.contains_interior(x, house.position[1], z))
        .expect("the visit target is inside a building")
        .position[1];
    [studio.game.x, studio.game.height, studio.game.z] = [x, floor, z];
    let _ = ctx.run(egui::RawInput::default(), |ctx| studio.advance(ctx));
    assert_eq!(
        studio.story_session.active_destination(&studio.scene.story),
        None
    );
    assert!(
        studio
            .story_session
            .objective(&studio.scene.story)
            .unwrap()
            .contains(&completion)
    );
}
