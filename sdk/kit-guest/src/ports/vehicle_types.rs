//! The vehicles of the physics port (`locomotion.vehicle`, docs/design/locomotion-vehicle.md): the body of an entity
//! becomes a vehicle of the engine's own kind — Rapier's ray-cast vehicle, Godot's `VehicleBody3D`, Unity's
//! `WheelCollider`, Unreal's Chaos Vehicle — carried by wheels, by floats on water, by hover pads, or by nothing
//! (space). The kit that drives it writes commands ([`VehicleInput`], forces in newtons, never a velocity) and reads
//! what the engine reports ([`VehicleState`]). One description for every wire: the kitcall codec, the orders of the
//! link, the module's entry (words of f64).

use super::V3;

/// The most contact points (wheels, floats, pads) of one vehicle.
pub const VEHICLE_MAX_WHEELS: usize = 8;
/// The words of a vehicle, its points, its commands, its report: contracts `abi.physics.vehicle`, `vehicle_wheel`,
/// `vehicle_input`, `vehicle_state`, `vehicle_wheel_state` (core/abi/contracts/), every index generated.
pub use gripforge_contracts::{
    abi_physics_vehicle as vehicle_words, abi_physics_vehicle_input as vehicle_input_words,
    abi_physics_vehicle_state as vehicle_state_words,
    abi_physics_vehicle_wheel as vehicle_wheel_words,
    abi_physics_vehicle_wheel_state as vehicle_wheel_state_words,
};
/// The words of a vehicle description before its points.
pub const VEHICLE_HEAD_WORDS: usize = vehicle_words::WORDS;
/// The words of one contact point.
pub const VEHICLE_WHEEL_WORDS: usize = vehicle_wheel_words::WORDS;
/// The words of the commands of a vehicle.
pub const VEHICLE_INPUT_WORDS: usize = vehicle_input_words::WORDS;
/// The words of what a vehicle reports: its head, then the words of each point.
pub const VEHICLE_STATE_WORDS: usize =
    vehicle_state_words::WORDS + vehicle_wheel_state_words::WORDS * VEHICLE_MAX_WHEELS;

/// What carries a vehicle (its number on every wire).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VehicleSupport {
    /// Wheels on a suspension: the engine's own wheeled vehicle.
    Wheels = 0,
    /// Floats on water: each point pushed up by how deep it is under `water_level`.
    Float = 1,
    /// Hover pads: each point held `hover_height` above what is under it by a spring.
    Hover = 2,
    /// Nothing (space): only the thrust moves it.
    None = 3,
}

impl VehicleSupport {
    /// The support of a name (`wheels`, `float`, `hover`, `none`).
    pub fn named(name: &str) -> Option<VehicleSupport> {
        Some(match name {
            "wheels" => VehicleSupport::Wheels,
            "float" => VehicleSupport::Float,
            "hover" => VehicleSupport::Hover,
            "none" => VehicleSupport::None,
            _ => return None,
        })
    }
    fn of(n: f64) -> Option<VehicleSupport> {
        let all = [
            VehicleSupport::Wheels,
            VehicleSupport::Float,
            VehicleSupport::Hover,
            VehicleSupport::None,
        ];
        all.get(n as usize).copied().filter(|_| n >= 0.0)
    }
}

/// One contact point of a vehicle: a wheel, a float or a hover pad, in the body's frame (+Z forward, +Y up).
#[derive(Clone, Debug, PartialEq)]
pub struct VehicleWheel {
    /// Where the suspension is fixed to the body (a wheel), or where the point pushes (a float, a pad).
    pub at: V3,
    pub radius: f64,
    /// How much of the steering angle the wheel takes (1 a front wheel, 0 a rear one, -1 a rear-steering one).
    pub steer: f64,
    /// How much of the engine force the wheel takes (the engine force is shared among the driven wheels).
    pub drive: f64,
    /// How much of the brake force the wheel takes.
    pub brake: f64,
    /// How much of the handbrake force the wheel takes.
    pub handbrake: f64,
}

impl VehicleWheel {
    /// A wheel at `at`, neither steered, driven nor braked.
    pub fn new(at: V3, radius: f64) -> VehicleWheel {
        VehicleWheel {
            at,
            radius,
            steer: 0.0,
            drive: 0.0,
            brake: 1.0,
            handbrake: 0.0,
        }
    }
}

/// A vehicle: what the engine needs once to build it (the tuning of the genre, resolved by the kit).
#[derive(Clone, Debug, PartialEq)]
pub struct VehicleDesc {
    pub support: VehicleSupport,
    /// Wheels: the length of a suspension at rest and how far it travels (m).
    pub rest_length: f64,
    pub travel: f64,
    /// Wheels: how stiff a suspension is, and its damping in compression and relaxation.
    pub stiffness: f64,
    pub compression: f64,
    pub relaxation: f64,
    /// Wheels: how much a tyre holds before it slips, and the side grip.
    pub friction_slip: f64,
    pub side_grip: f64,
    /// Wheels: the most force one suspension gives (N).
    pub max_suspension_force: f64,
    /// Hover: the height held above the ground (m), the spring's stiffness and damping (per kg of the body, over the
    /// weight each pad carries at rest).
    pub hover_height: f64,
    pub hover_stiffness: f64,
    pub hover_damping: f64,
    /// Float: how many times its weight the floats carry when fully under (2: a hull rides half sunk), the water's
    /// level (m), its drag (per kg of the body).
    pub buoyancy: f64,
    pub water_level: f64,
    pub water_drag: f64,
    /// Float and hover: how hard the sideways slide is damped (per kg of the body; wheels use their tyres).
    pub side_drag: f64,
    /// Where the mass of the body sits, in its frame (a low centre keeps a car on its wheels).
    pub center_of_mass: V3,
    pub wheels: Vec<VehicleWheel>,
}

impl VehicleDesc {
    /// A vehicle of `support` with the defaults of a small car, and no point yet.
    pub fn new(support: VehicleSupport) -> VehicleDesc {
        VehicleDesc {
            support,
            rest_length: 0.35,
            travel: 0.25,
            stiffness: 30.0,
            compression: 2.3,
            relaxation: 4.4,
            friction_slip: 2.5,
            side_grip: 1.0,
            max_suspension_force: 60000.0,
            hover_height: 0.6,
            hover_stiffness: 40.0,
            hover_damping: 6.0,
            buoyancy: 2.0,
            water_level: 0.0,
            water_drag: 0.8,
            side_drag: 1.5,
            center_of_mass: [0.0, -0.3, 0.0],
            wheels: Vec::new(),
        }
    }

    /// Its words: support, rest length, travel, stiffness, compression, relaxation, friction slip, side grip, max
    /// suspension force, hover height, hover stiffness, hover damping, buoyancy, water level, water drag, side drag,
    /// centre of mass (3); then per point: at (3), radius, steer, drive, brake, handbrake.
    pub fn words(&self) -> Vec<f64> {
        use vehicle_words::at;
        let n = self.wheels.len().min(VEHICLE_MAX_WHEELS);
        let mut w = vec![0.0; VEHICLE_HEAD_WORDS + n * VEHICLE_WHEEL_WORDS];
        w[at::SUPPORT] = self.support as u8 as f64;
        w[at::REST_LENGTH] = self.rest_length;
        w[at::TRAVEL] = self.travel;
        w[at::STIFFNESS] = self.stiffness;
        w[at::COMPRESSION] = self.compression;
        w[at::RELAXATION] = self.relaxation;
        w[at::FRICTION_SLIP] = self.friction_slip;
        w[at::SIDE_GRIP] = self.side_grip;
        w[at::MAX_SUSPENSION_FORCE] = self.max_suspension_force;
        w[at::HOVER_HEIGHT] = self.hover_height;
        w[at::HOVER_STIFFNESS] = self.hover_stiffness;
        w[at::HOVER_DAMPING] = self.hover_damping;
        w[at::BUOYANCY] = self.buoyancy;
        w[at::WATER_LEVEL] = self.water_level;
        w[at::WATER_DRAG] = self.water_drag;
        w[at::SIDE_DRAG] = self.side_drag;
        w[at::CENTER_OF_MASS..at::CENTER_OF_MASS + 3].copy_from_slice(&self.center_of_mass);
        for (p, o) in self.wheels.iter().zip(
            w[VEHICLE_HEAD_WORDS..]
                .as_chunks_mut::<{ VEHICLE_WHEEL_WORDS }>()
                .0
                .iter_mut(),
        ) {
            use vehicle_wheel_words::at;
            o[at::POSITION..at::POSITION + 3].copy_from_slice(&p.at);
            o[at::RADIUS] = p.radius;
            o[at::STEER] = p.steer;
            o[at::DRIVE] = p.drive;
            o[at::BRAKE] = p.brake;
            o[at::HANDBRAKE] = p.handbrake;
        }
        w
    }

    /// The description of its words (`None`: too few words, an unknown support, or a number that is not finite).
    pub fn of_words(w: &[f64]) -> Option<VehicleDesc> {
        use vehicle_words as h;
        if h::check(w).is_err() || w.iter().any(|x| !x.is_finite()) {
            return None;
        }
        let wheels = w[VEHICLE_HEAD_WORDS..]
            .as_chunks::<{ VEHICLE_WHEEL_WORDS }>()
            .0
            .iter()
            .take(VEHICLE_MAX_WHEELS)
            .map(|p| VehicleWheel {
                at: vehicle_wheel_words::position(p),
                radius: vehicle_wheel_words::radius(p),
                steer: vehicle_wheel_words::steer(p),
                drive: vehicle_wheel_words::drive(p),
                brake: vehicle_wheel_words::brake(p),
                handbrake: vehicle_wheel_words::handbrake(p),
            })
            .collect();
        Some(VehicleDesc {
            support: VehicleSupport::of(h::support(w))?,
            rest_length: h::rest_length(w),
            travel: h::travel(w),
            stiffness: h::stiffness(w),
            compression: h::compression(w),
            relaxation: h::relaxation(w),
            friction_slip: h::friction_slip(w),
            side_grip: h::side_grip(w),
            max_suspension_force: h::max_suspension_force(w),
            hover_height: h::hover_height(w),
            hover_stiffness: h::hover_stiffness(w),
            hover_damping: h::hover_damping(w),
            buoyancy: h::buoyancy(w),
            water_level: h::water_level(w),
            water_drag: h::water_drag(w),
            side_drag: h::side_drag(w),
            center_of_mass: h::center_of_mass(w),
            wheels,
        })
    }
}

/// The commands of a vehicle until the next ones: forces and an angle, never a velocity (the engine integrates).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct VehicleInput {
    /// The engine force (N, negative backward), shared among the driven wheels by their `drive`.
    pub engine: f64,
    /// The brake force (N) of each wheel, times its `brake`.
    pub brake: f64,
    /// The handbrake force (N) of each wheel, times its `handbrake`.
    pub handbrake: f64,
    /// The steering angle (radians, positive to the left), times each wheel's `steer`.
    pub steer: f64,
    /// A force (N) and a torque (N·m) in the body's frame: what moves a vehicle without wheels (a boat's
    /// propeller, a hovercraft's fans, a ship's thrusters), and what tilts any of them.
    pub thrust: V3,
    pub torque: V3,
}

impl VehicleInput {
    /// Its words: engine, brake, handbrake, steer, thrust (3), torque (3), then two kept at 0.
    pub fn words(&self) -> [f64; VEHICLE_INPUT_WORDS] {
        use vehicle_input_words::at;
        let mut w = [0.0; VEHICLE_INPUT_WORDS];
        w[at::ENGINE] = self.engine;
        w[at::BRAKE] = self.brake;
        w[at::HANDBRAKE] = self.handbrake;
        w[at::STEER] = self.steer;
        w[at::THRUST..at::THRUST + 3].copy_from_slice(&self.thrust);
        w[at::TORQUE..at::TORQUE + 3].copy_from_slice(&self.torque);
        w
    }

    /// The commands of their words (the missing ones at 0, their default; a number that is not finite is 0).
    pub fn of_words(w: &[f64]) -> VehicleInput {
        use vehicle_input_words as i;
        let n = |x: f64| if x.is_finite() { x } else { 0.0 };
        VehicleInput {
            engine: n(i::engine(w)),
            brake: n(i::brake(w)),
            handbrake: n(i::handbrake(w)),
            steer: n(i::steer(w)),
            thrust: i::thrust(w).map(n),
            torque: i::torque(w).map(n),
        }
    }
}

/// What a contact point reports after a step.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct VehicleWheelState {
    /// The point touches the ground (a wheel, a pad) or the water (a float).
    pub contact: bool,
    /// The length of the suspension (a wheel), the height above the ground (a pad), the depth (a float), m.
    pub length: f64,
    /// The steering angle the wheel took (radians).
    pub steer: f64,
    /// How far the wheel turned on its axle (radians, accumulated).
    pub rotation: f64,
}

/// What a vehicle reports after a step.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct VehicleState {
    /// m/s along the body's forward (+Z), negative backward.
    pub forward_speed: f64,
    /// m/s, any direction.
    pub speed: f64,
    /// How many points touch.
    pub contacts: u32,
    pub wheels: Vec<VehicleWheelState>,
}

impl VehicleState {
    /// Its words: forward speed, speed, contacts, points; then per point: contact, length, steer, rotation.
    pub fn words(&self) -> [f64; VEHICLE_STATE_WORDS] {
        use vehicle_state_words::at;
        let mut w = [0.0; VEHICLE_STATE_WORDS];
        w[at::FORWARD_SPEED] = self.forward_speed;
        w[at::SPEED] = self.speed;
        w[at::CONTACTS] = f64::from(self.contacts);
        let n = self.wheels.len().min(VEHICLE_MAX_WHEELS);
        w[at::POINTS] = n as f64;
        let points = w[vehicle_state_words::WORDS..]
            .as_chunks_mut::<{ vehicle_wheel_state_words::WORDS }>()
            .0
            .iter_mut();
        for (p, o) in self.wheels.iter().take(n).zip(points) {
            use vehicle_wheel_state_words::at;
            o[at::CONTACT] = f64::from(p.contact as u8);
            o[at::LENGTH] = p.length;
            o[at::STEER] = p.steer;
            o[at::ROTATION] = p.rotation;
        }
        w
    }

    /// The state of its words (`None`: too few words).
    pub fn of_words(w: &[f64]) -> Option<VehicleState> {
        use vehicle_state_words as s;
        let w = w.get(..VEHICLE_STATE_WORDS)?;
        let n = (s::points(w).max(0.0) as usize).min(VEHICLE_MAX_WHEELS);
        let wheels = w[s::WORDS..]
            .as_chunks::<{ vehicle_wheel_state_words::WORDS }>()
            .0
            .iter()
            .take(n)
            .map(|p| VehicleWheelState {
                contact: vehicle_wheel_state_words::contact(p) != 0.0,
                length: vehicle_wheel_state_words::length(p),
                steer: vehicle_wheel_state_words::steer(p),
                rotation: vehicle_wheel_state_words::rotation(p),
            })
            .collect();
        Some(VehicleState {
            forward_speed: s::forward_speed(w),
            speed: s::speed(w),
            contacts: s::contacts(w).max(0.0) as u32,
            wheels,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_vehicle_reads_back_from_its_words() {
        let mut d = VehicleDesc::new(VehicleSupport::Hover);
        d.hover_height = 0.8;
        let mut w = VehicleWheel::new([0.8, -0.2, 1.2], 0.3);
        (w.steer, w.drive) = (1.0, 0.5);
        d.wheels = vec![w.clone(), w];
        assert_eq!(VehicleDesc::of_words(&d.words()), Some(d));
        assert_eq!(VehicleDesc::of_words(&[9.0; VEHICLE_HEAD_WORDS]), None);
        assert_eq!(VehicleDesc::of_words(&[0.0; 3]), None);
        let i = VehicleInput {
            engine: 900.0,
            steer: -0.3,
            thrust: [0.0, 1.0, 2.0],
            ..VehicleInput::default()
        };
        assert_eq!(VehicleInput::of_words(&i.words()), i);
        let s = VehicleState {
            forward_speed: 3.0,
            speed: 3.5,
            contacts: 1,
            wheels: vec![VehicleWheelState {
                contact: true,
                length: 0.3,
                steer: 0.1,
                rotation: 2.0,
            }],
        };
        assert_eq!(VehicleState::of_words(&s.words()), Some(s));
    }
}
