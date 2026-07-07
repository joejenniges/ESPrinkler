//! Physical valve actuation.
//!
//! - AC relay: one output per valve, level-held while open (relay NO contact
//!   feeds 24VAC to the valve).
//! - DC latching solenoid: two outputs per valve; a brief pulse on the open line
//!   opens it, a pulse on the close line closes it (holds position between
//!   pulses), so we only pulse on state changes.
//!
//! Control logic (manual/schedules) is deferred; for now we build the drivers,
//! initialise every valve closed, and expose `apply()` for later use.

use esp_idf_svc::hal::delay::FreeRtos;
use esp_idf_svc::hal::gpio::{AnyIOPin, Output, PinDriver};

use crate::config::Valve;

/// Settle gap between consecutive latching valves during the boot close-all, so
/// their inrush currents don't stack.
const STARTUP_CLOSE_GAP_MS: u32 = 100;

pub enum Actuator {
    Ac {
        pin: PinDriver<'static, Output>,
        active_low: bool,
    },
    Latching {
        open: PinDriver<'static, Output>,
        close: PinDriver<'static, Output>,
        pulse_ms: u32,
    },
}

/// Build an output driver for a GPIO chosen at runtime by number.
///
/// SAFETY: `AnyIOPin::steal` bypasses the typed peripheral singletons; the
/// caller must claim each GPIO exactly once. Config assigns distinct pins.
fn output(pin: u8) -> anyhow::Result<PinDriver<'static, Output>> {
    let any = unsafe { AnyIOPin::steal(pin) };
    Ok(PinDriver::output(any)?)
}

pub fn build(valves: &[Valve]) -> anyhow::Result<Vec<Actuator>> {
    let mut actuators = Vec::with_capacity(valves.len());

    for v in valves {
        let actuator = match v.kind.as_str() {
            "latching" => {
                let open_pin = v
                    .open_pin
                    .ok_or_else(|| anyhow::anyhow!("valve '{}': latching needs open_pin", v.name))?;
                let close_pin = v.close_pin.ok_or_else(|| {
                    anyhow::anyhow!("valve '{}': latching needs close_pin", v.name)
                })?;
                let mut open = output(open_pin)?;
                let mut close = output(close_pin)?;
                open.set_low()?;
                close.set_low()?;
                Actuator::Latching {
                    open,
                    close,
                    pulse_ms: v.pulse_ms,
                }
            }
            other => {
                if other != "ac" {
                    log::warn!("valve '{}': unknown type '{other}', treating as ac", v.name);
                }
                let pin = v
                    .pin
                    .ok_or_else(|| anyhow::anyhow!("valve '{}': ac needs pin", v.name))?;
                let mut p = output(pin)?;
                // Start de-energized (valve closed).
                if v.active_low {
                    p.set_high()?;
                } else {
                    p.set_low()?;
                }
                Actuator::Ac {
                    pin: p,
                    active_low: v.active_low,
                }
            }
        };
        actuators.push(actuator);
    }

    // Latching solenoids power up in an unknown position; pulse them all closed,
    // staggered so inrush currents don't stack.
    let mut first = true;
    for actuator in actuators.iter_mut() {
        if let Actuator::Latching {
            close, pulse_ms, ..
        } = actuator
        {
            if !first {
                FreeRtos::delay_ms(STARTUP_CLOSE_GAP_MS);
            }
            first = false;
            close.set_high()?;
            FreeRtos::delay_ms(*pulse_ms);
            close.set_low()?;
        }
    }

    Ok(actuators)
}

impl Actuator {
    /// Drive this valve open (`want == true`) or closed.
    pub fn apply(&mut self, want: bool) -> anyhow::Result<()> {
        match self {
            Actuator::Ac { pin, active_low } => {
                if want ^ *active_low {
                    pin.set_high()?;
                } else {
                    pin.set_low()?;
                }
            }
            Actuator::Latching {
                open,
                close,
                pulse_ms,
            } => {
                let line = if want { open } else { close };
                line.set_high()?;
                FreeRtos::delay_ms(*pulse_ms);
                line.set_low()?;
            }
        }
        Ok(())
    }
}
