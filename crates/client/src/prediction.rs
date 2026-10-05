//! Reconcile at the server's acknowledged input, then replay newer movement.
//! Each side runs the same controller with the same input and duration.

use std::collections::VecDeque;

#[cfg(test)]
use rubblekin_core::physics::move_character_with_obstacles;
use rubblekin_core::{
    airships::{AirshipNetwork, AirshipRide},
    physics::{Body, MoveInput, move_character_with_airships},
    protocol::{ClientMessage, MAX_INPUT_DT, PlayerSnapshot},
    world::World,
};

const MAX_PENDING_INPUTS: usize = 512;
const MAX_PENDING_SECONDS: f32 = 2.0;

struct PendingInput {
    sequence: u64,
    input: MoveInput,
    dt: f32,
    time: Option<f64>,
}

#[derive(Default)]
pub struct Prediction {
    pending: VecDeque<PendingInput>,
    pending_seconds: f32,
    sequence: u64,
    acknowledged: u64,
}

impl Prediction {
    #[cfg(test)]
    pub fn advance(
        &mut self,
        world: &World,
        body: &mut Body,
        input: MoveInput,
        yaw: f32,
        dt: f32,
        obstacles: &[[f32; 3]],
    ) -> Result<ClientMessage, &'static str> {
        let message = self.record(input, yaw, dt, None)?;
        move_character_with_obstacles(world, body, input, dt, obstacles);
        Ok(message)
    }

    /// The same moving-platform controller handles ordinary ground travel,
    /// physical boarding, deck walking, jumps and falls on both peers.
    #[allow(clippy::too_many_arguments)]
    pub fn advance_airships(
        &mut self,
        world: &World,
        body: &mut Body,
        input: MoveInput,
        yaw: f32,
        dt: f32,
        obstacles: &[[f32; 3]],
        network: &AirshipNetwork,
        time: f64,
        ride: &mut Option<AirshipRide>,
        local: &mut Option<[f32; 3]>,
    ) -> Result<ClientMessage, &'static str> {
        let message = self.record(input, yaw, dt, Some(time))?;
        move_character_with_airships(
            world, body, input, dt, obstacles, network, time, ride, local,
        );
        Ok(message)
    }

    fn record(
        &mut self,
        input: MoveInput,
        yaw: f32,
        dt: f32,
        time: Option<f64>,
    ) -> Result<ClientMessage, &'static str> {
        if !dt.is_finite() || dt <= 0.0 || dt > MAX_INPUT_DT {
            return Err("Invalid movement duration");
        }
        if self.pending.len() >= MAX_PENDING_INPUTS
            || self.pending_seconds + dt > MAX_PENDING_SECONDS
        {
            return Err("Server stopped acknowledging movement; reconnect to resynchronize");
        }
        self.sequence = self
            .sequence
            .checked_add(1)
            .ok_or("Input sequence exhausted")?;
        self.pending.push_back(PendingInput {
            sequence: self.sequence,
            input,
            dt,
            time,
        });
        self.pending_seconds += dt;
        Ok(ClientMessage::Input {
            sequence: self.sequence,
            input,
            yaw,
            dt,
        })
    }

    #[cfg(test)]
    pub fn reconcile(
        &mut self,
        world: &World,
        body: &mut Body,
        authoritative: &PlayerSnapshot,
        obstacles: &[[f32; 3]],
    ) -> Result<(), &'static str> {
        self.acknowledge(authoritative.last_input_sequence)?;
        *body = authoritative.body.clone();
        if authoritative.ride.is_none() {
            for input in &self.pending {
                move_character_with_obstacles(world, body, input.input, input.dt, obstacles);
            }
        }
        Ok(())
    }
    #[allow(clippy::too_many_arguments)]
    pub fn reconcile_airships(
        &mut self,
        world: &World,
        body: &mut Body,
        authoritative: &PlayerSnapshot,
        obstacles: &[[f32; 3]],
        network: &AirshipNetwork,
        world_time: f64,
        ride: &mut Option<AirshipRide>,
        local: &mut Option<[f32; 3]>,
    ) -> Result<(), &'static str> {
        self.acknowledge(authoritative.last_input_sequence)?;
        *body = authoritative.body.clone();
        *ride = authoritative.ride;
        *local = authoritative.deck_position;
        for pending in &self.pending {
            move_character_with_airships(
                world,
                body,
                pending.input,
                pending.dt,
                obstacles,
                network,
                pending.time.unwrap_or(world_time).max(world_time),
                ride,
                local,
            );
        }
        Ok(())
    }

    fn acknowledge(&mut self, acknowledged: u64) -> Result<(), &'static str> {
        if acknowledged < self.acknowledged || acknowledged > self.sequence {
            return Err("Server sent an invalid movement acknowledgment");
        }
        self.acknowledged = acknowledged;
        while self
            .pending
            .front()
            .is_some_and(|input| input.sequence <= acknowledged)
        {
            let input = self.pending.pop_front().unwrap();
            self.pending_seconds = (self.pending_seconds - input.dt).max(0.0);
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "prediction_tests.rs"]
mod tests;
