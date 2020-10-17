use bevy::{input::system::exit_on_esc_system, prelude::*};
use bevy_prototype_character_controller::{
    controller::{CharacterController, CharacterControllerPlugin, Mass},
    events::{ForceEvent, ImpulseEvent, TranslationEvent, YawEvent},
    look::LookDirection,
};
use bevy_prototype_physx::*;
use clap::{arg_enum, value_t};
use rand::Rng;

// Take a look at example_utils/utils.rs for details!
#[path = "../example_utils/utils.rs"]
mod utils;
use utils::*;

arg_enum! {
    #[derive(PartialEq, Debug)]
    pub enum ControllerType {
        KinematicTranslation,
        DynamicImpulse,
        DynamicForce,
    }
}

impl Default for ControllerType {
    fn default() -> Self {
        ControllerType::DynamicForce
    }
}

pub struct KinematicYawTag;

fn main() {
    let matches = clap::App::new("Bevy PhysX 3D Character Controller")
        .arg(
            clap::Arg::from_usage("<type> Controller type. ")
                .possible_values(&ControllerType::variants())
                .case_insensitive(true)
                .default_value("DynamicForce"),
        )
        .get_matches();
    let controller_type =
        value_t!(matches.value_of("type"), ControllerType).unwrap_or(ControllerType::DynamicForce);

    let mut app = App::build();

    // Generic
    app.add_resource(ClearColor(Color::hex("101010").unwrap()))
        .add_resource(Msaa { samples: 4 })
        .add_default_plugins()
        .add_system(exit_on_esc_system.system())
        // Character Controller
        .add_plugin(CharacterControllerPlugin)
        .init_resource::<ControllerEvents>()
        // PhysX
        .add_plugin(PhysXPlugin)
        // Character controller adaptations for PhysX
        .add_system(create_mass.system())
        .add_system(constrain_rotation.system())
        .add_system_to_stage_front(bevy::app::stage::PRE_UPDATE, body_to_velocity.system());

    // IMPORTANT: The impulse/force systems MUST run before the physics simulation step, so they
    // either need to be added to the end of PRE_UPDATE or the beginning of UPDATE
    if controller_type == ControllerType::KinematicTranslation {
        // Option A. Apply translations (changes in position)
        app.add_system_to_stage_front(
            bevy::app::stage::UPDATE,
            controller_to_physx_kinematic.system(),
        );
    } else if controller_type == ControllerType::DynamicImpulse {
        // Option B. Apply impulses (changes in momentum)
        app.add_system_to_stage_front(
            bevy::app::stage::UPDATE,
            controller_to_physx_dynamic_impulse.system(),
        );
    } else {
        // Option C. Apply forces (rate of change of momentum)
        app.add_system_to_stage_front(
            bevy::app::stage::UPDATE,
            controller_to_physx_dynamic_force.system(),
        );
    }

    // The yaw needs to be applied to the rigid body so this system has to
    // be implemented for the physics engine in question
    if controller_type == ControllerType::KinematicTranslation {
        app.add_system_to_stage_front(
            bevy::app::stage::UPDATE,
            controller_to_physx_kinematic_yaw.system(),
        );
    } else {
        app.add_system_to_stage_front(
            bevy::app::stage::UPDATE,
            controller_to_physx_dynamic_yaw.system(),
        );
    }

    app
        // Controllers generally all want to pitch in the same way
        .add_system_to_stage_front(bevy::app::stage::UPDATE, controller_to_pitch.system())
        // Specific to this demo
        .init_resource::<CharacterSettings>()
        .add_resource(controller_type)
        .add_startup_system(spawn_world.system())
        .add_startup_system(spawn_character.system())
        .run();
}

pub fn spawn_world(
    mut commands: Commands,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let cube = meshes.add(Mesh::from(shape::Cube { size: 0.5 }));

    // Light
    commands.spawn(LightComponents {
        transform: Transform::from_translation(Vec3::new(-15.0, 10.0, -15.0)),
        ..Default::default()
    });

    // Ground cuboid
    let grey = materials.add(Color::hex("808080").unwrap().into());
    let box_xz = 200.0;
    let box_y = 1.0;
    commands
        .spawn(PbrComponents {
            material: grey,
            mesh: cube,
            transform: Transform::new(Mat4::from_scale_rotation_translation(
                Vec3::new(box_xz, box_y, box_xz),
                Quat::identity(),
                Vec3::zero(),
            )),
            ..Default::default()
        })
        .with_bundle((
            PhysXMaterialDesc {
                static_friction: 0.5,
                dynamic_friction: 0.5,
                restitution: 0.6,
            },
            PhysXColliderDesc::Box(0.5 * box_xz, 0.5 * box_y, 0.5 * box_xz),
            PhysXRigidBodyDesc::Static,
        ));

    // Cubes for some kind of reference in the scene to make it easy to see
    // what is happening
    let teal = materials.add(Color::hex("008080").unwrap().into());
    let cube_scale = 1.0;
    let mut rng = rand::thread_rng();
    for _ in 0..20 {
        let x = rng.gen_range(-10.0, 10.0);
        let z = rng.gen_range(-10.0, 10.0);
        commands
            .spawn(PbrComponents {
                material: teal,
                mesh: cube,
                transform: Transform::from_translation_rotation_scale(
                    Vec3::new(x, 0.5 * (cube_scale + box_y), z),
                    Quat::identity(),
                    cube_scale,
                ),
                ..Default::default()
            })
            .with_bundle((
                PhysXMaterialDesc {
                    static_friction: 0.1,
                    dynamic_friction: 0.4,
                    restitution: 0.6,
                },
                PhysXColliderDesc::Box(0.5 * cube_scale, 0.5 * cube_scale, 0.5 * cube_scale),
                PhysXRigidBodyDesc::Dynamic {
                    density: 10.0,
                    angular_damping: 0.5,
                },
            ));
    }
}

pub fn spawn_character(
    mut commands: Commands,
    controller_type: Res<ControllerType>,
    character_settings: Res<CharacterSettings>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let box_y = 1.0;
    commands.spawn((
        GlobalTransform::identity(),
        Transform::from_translation(Vec3::new(
            0.0,
            0.5 * (box_y + character_settings.scale.y()),
            0.0,
        )),
        CharacterController::default(),
        PhysXMaterialDesc {
            static_friction: 0.5,
            dynamic_friction: 0.5,
            restitution: 0.0,
        },
        BodyTag,
    ));

    if *controller_type == ControllerType::KinematicTranslation {
        commands
            .with_bundle((
                Mass::new(80.0),
                PhysXCapsuleControllerDesc {
                    height: character_settings.scale.y(),
                    radius: character_settings
                        .scale
                        .x()
                        .max(character_settings.scale.z()),
                    step_offset: 0.5,
                },
            ))
            .with_children(|body| {
                body.spawn((
                    GlobalTransform::identity(),
                    Transform::identity(),
                    KinematicYawTag,
                ))
                .with_children(|kinematic_yaw| {
                    spawn_body_children(
                        kinematic_yaw,
                        &controller_type,
                        &character_settings,
                        box_y,
                        &mut materials,
                        &mut meshes,
                    );
                });
            });
    } else {
        commands
            .with_bundle((
                PhysXColliderDesc::Capsule(
                    0.5 * character_settings
                        .scale
                        .x()
                        .max(character_settings.scale.z()),
                    character_settings.scale.y(),
                ),
                PhysXRigidBodyDesc::Dynamic {
                    density: 200.0,
                    angular_damping: 0.5,
                },
            ))
            .with_children(|body| {
                spawn_body_children(
                    body,
                    &controller_type,
                    &character_settings,
                    box_y,
                    &mut materials,
                    &mut meshes,
                );
            });
    }
}

fn spawn_body_children(
    body: &mut ChildBuilder,
    controller_type: &Res<ControllerType>,
    character_settings: &Res<CharacterSettings>,
    box_y: f32,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    meshes: &mut ResMut<Assets<Mesh>>,
) {
    let cube = meshes.add(Mesh::from(shape::Cube { size: 0.5 }));
    // Character
    let red = materials.add(Color::hex("800000").unwrap().into());
    let (body_translation, head_translation) = if **controller_type
        == ControllerType::KinematicTranslation
    {
        (
            -0.5 * character_settings.head_scale * Vec3::unit_y(),
            0.5 * (character_settings.scale.y() - character_settings.head_scale) * Vec3::unit_y(),
        )
    } else {
        (
            0.5 * box_y * Vec3::unit_y(),
            0.5 * (box_y + character_settings.scale.y()) * Vec3::unit_y(),
        )
    };
    body.spawn(PbrComponents {
        material: red,
        mesh: cube,
        transform: Transform::new(Mat4::from_scale_rotation_translation(
            character_settings.scale - character_settings.head_scale * Vec3::unit_y(),
            Quat::identity(),
            body_translation,
        )),
        ..Default::default()
    })
    .spawn((
        GlobalTransform::identity(),
        Transform::from_translation_rotation(
            head_translation,
            Quat::from_rotation_y(character_settings.head_yaw),
        ),
        HeadTag,
    ))
    .with_children(|head| {
        head.spawn(PbrComponents {
            material: red,
            mesh: cube,
            transform: Transform::from_scale(character_settings.head_scale),
            ..Default::default()
        })
        .spawn(Camera3dComponents {
            transform: Transform::new(Mat4::face_toward(
                character_settings.follow_offset,
                character_settings.focal_point,
                Vec3::unit_y(),
            )),
            ..Default::default()
        })
        .with(LookDirection::default())
        .with(CameraTag);
    });
}

pub struct ConstrainedTag;

pub fn constrain_rotation(
    mut commands: Commands,
    mut physx: ResMut<PhysX>,
    mut query: Query<
        Without<ConstrainedTag, With<BodyTag, (Entity, &PhysXDynamicRigidBodyHandle)>>,
    >,
) {
    for (entity, body_handle) in &mut query.iter() {
        let mut body = physx
            .scene
            .get_dynamic_mut(body_handle.0)
            .expect("Failed to get dynamic rigid body");
        body.set_mass_space_inertia_tensor(Vec3::zero());
        commands.insert_one(entity, ConstrainedTag);
    }
}

pub fn create_mass(
    mut commands: Commands,
    physx: Res<PhysX>,
    mut query: Query<Without<Mass, (Entity, &PhysXDynamicRigidBodyHandle)>>,
) {
    for (entity, body_handle) in &mut query.iter() {
        let body = physx
            .scene
            .get_dynamic(body_handle.0)
            .expect("Failed to get dynamic rigid body");
        commands.insert_one(entity, Mass::new(body.get_mass()));
    }
}

pub fn body_to_velocity(
    physx: Res<PhysX>,
    _body: &BodyTag,
    body_handle: &PhysXDynamicRigidBodyHandle,
    mut controller: Mut<CharacterController>,
) {
    let body = physx
        .scene
        .get_dynamic(body_handle.0)
        .expect("Failed to get dynamic rigid body");
    controller.velocity = body.get_linear_velocity();
}

pub fn controller_to_physx_kinematic(
    translations: Res<Events<TranslationEvent>>,
    character_settings: Res<CharacterSettings>,
    mut reader: ResMut<ControllerEvents>,
    mut _physx: ResMut<PhysX>, // For synchronization
    _body: &BodyTag,
    mut physx_controller: Mut<PhysXController>,
    mut transform: Mut<Transform>,
    mut controller: Mut<CharacterController>,
) {
    let mut translation = Vec3::zero();
    for event in reader.translations.iter(&translations) {
        translation += **event;
    }
    // NOTE: This is just an example to stop falling past the initial body height
    // With a physics engine you would indicate that the body has collided with
    // something and should stop, depending on how your game works.
    let min_y = 0.5 * (1.0 + character_settings.scale.y());
    let position = physx_controller.get_position();
    if position.y() + translation.y() < min_y {
        *translation.y_mut() = min_y - position.y();
        controller.jumping = false;
    }
    let new_position = position + translation;
    physx_controller.set_position(new_position);
    transform.translate(translation);
}

pub fn controller_to_physx_dynamic_impulse(
    impulses: Res<Events<ImpulseEvent>>,
    mut reader: ResMut<ControllerEvents>,
    mut physx: ResMut<PhysX>,
    _body: &BodyTag,
    body_handle: &PhysXDynamicRigidBodyHandle,
) {
    let mut impulse = Vec3::zero();
    for event in reader.impulses.iter(&impulses) {
        impulse += **event;
    }

    if impulse.length_squared() > 1E-6 {
        let mut body = physx
            .scene
            .get_dynamic_mut(body_handle.0)
            .expect("Failed to get dynamic rigid body");
        body.add_force(impulse, physx::rigid_body::ForceMode::Impulse, true);
    }
}

pub fn controller_to_physx_dynamic_force(
    forces: Res<Events<ForceEvent>>,
    mut reader: ResMut<ControllerEvents>,
    mut physx: ResMut<PhysX>,
    _body: &BodyTag,
    body_handle: &PhysXDynamicRigidBodyHandle,
) {
    let mut force = Vec3::zero();
    for event in reader.forces.iter(&forces) {
        force += **event;
    }

    if force.length_squared() > 1E-6 {
        let mut body = physx
            .scene
            .get_dynamic_mut(body_handle.0)
            .expect("Failed to get dynamic rigid body");
        body.add_force(force, physx::rigid_body::ForceMode::Force, true);
    }
}

pub fn controller_to_physx_kinematic_yaw(
    mut reader: ResMut<ControllerEvents>,
    yaws: Res<Events<YawEvent>>,
    _yaw: &KinematicYawTag,
    mut transform: Mut<Transform>,
) {
    let mut yaw = None;
    for event in reader.yaws.iter(&yaws) {
        yaw = Some(**event);
    }
    if let Some(yaw) = yaw {
        transform.set_rotation(Quat::from_rotation_y(yaw));
    }
}

pub fn controller_to_physx_dynamic_yaw(
    mut reader: ResMut<ControllerEvents>,
    yaws: Res<Events<YawEvent>>,
    mut physx: ResMut<PhysX>,
    _body: &BodyTag,
    body_handle: &PhysXDynamicRigidBodyHandle,
) {
    let mut yaw = None;
    for event in reader.yaws.iter(&yaws) {
        yaw = Some(**event);
    }
    if let Some(yaw) = yaw {
        let mut body = physx
            .scene
            .get_dynamic_mut(body_handle.0)
            .expect("Failed to get dynamic rigid body");
        let translation = body.get_global_pose().w_axis().truncate().into();
        body.set_global_pose(
            Mat4::from_rotation_translation(Quat::from_rotation_y(yaw), translation),
            true,
        );
    }
}
