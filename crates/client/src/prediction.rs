//! Reconcile at the server's acknowledged input, then replay newer movement.
//! Each side runs the same controller with the same input and duration.

use std::collections::VecDeque;

use rubblekin_core::{
    physics::{Body, MoveInput, move_character_with_obstacles},
    protocol::{ClientMessage, MAX_INPUT_DT, PlayerSnapshot},
    world::World,
};

const MAX_PENDING_INPUTS: usize = 512;
const MAX_PENDING_SECONDS: f32 = 2.0;

struct PendingInput {
    sequence: u64,
    input: MoveInput,
    dt: f32,
}

#[derive(Default)]
pub struct Prediction {
    pending: VecDeque<PendingInput>,
    pending_seconds: f32,
    sequence: u64,
    acknowledged: u64,
}

impl Prediction {
    pub fn advance(
        &mut self,
        world: &World,
        body: &mut Body,
        input: MoveInput,
        yaw: f32,
        dt: f32,
        obstacles: &[[f32; 3]],
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
        move_character_with_obstacles(world, body, input, dt, obstacles);
        self.pending.push_back(PendingInput {
            sequence: self.sequence,
            input,
            dt,
        });
        self.pending_seconds += dt;
        Ok(ClientMessage::Input {
            sequence: self.sequence,
            input,
            yaw,
            dt,
        })
    }

    pub fn reconcile(
        &mut self,
        world: &World,
        body: &mut Body,
        authoritative: &PlayerSnapshot,
        obstacles: &[[f32; 3]],
    ) -> Result<(), &'static str> {
        let acknowledged = authoritative.last_input_sequence;
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
        *body = authoritative.body.clone();
        for input in &self.pending {
            move_character_with_obstacles(world, body, input.input, input.dt, obstacles);
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "prediction_tests.rs"]
mod tests;
