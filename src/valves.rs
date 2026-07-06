//! Physical valve actuation.
//!
//! - AC relay: one output per valve, level-held while watering (the relay's NO
//!   contact feeds 24VAC to the valve).
//! - DC latching solenoid: two outputs per valve; a brief pulse on the open
//!   line latches it on, a pulse on the close line latches it off. The solenoid
//!   holds position mechanically, so we only pulse on state changes.
//!
//! GPIO is driven from the main task, never the BLE host task: the write
//! callback only updates a desired-state bitmask, and this reconciler applies
//! it. That keeps the (blocking) latch pulses off the Bluetooth stack.

use esp_idf_svc::hal::delay::FreeRtos;
use esp_idf_svc::hal::gpio::{AnyIOPin, Output, PinDriver};

use crate::config::{Config, DriveMode};

/// Fixed settle gap between consecutive latching valves during the boot
/// close-all sequence, so their inrush currents don't stack and spike the
/// supply. Deliberately not configurable.
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
/// SAFETY: `AnyIOPin::steal` bypasses the typed peripheral singletons, so the
/// caller must ensure each GPIO number is claimed exactly once. Config assigns
/// distinct pins per valve; we never construct the same number twice.
fn output(pin: u8) -> anyhow::Result<PinDriver<'static, Output>> {
    let any = unsafe { AnyIOPin::steal(pin) };
    Ok(PinDriver::output(any)?)
}

/// Construct one actuator per valve and drive every valve to the closed state.
pub fn build(config: &Config) -> anyhow::Result<Vec<Actuator>> {
    let mut actuators = Vec::with_capacity(config.valves as usize);

    for i in 0..config.valves as usize {
        let actuator = match config.drive {
            DriveMode::Ac => {
                let mut pin = output(config.ac_pins[i])?;
                // Start de-energized (valve closed).
                if config.active_low {
                    pin.set_high()?;
                } else {
                    pin.set_low()?;
                }
                Actuator::Ac {
                    pin,
                    active_low: config.active_low,
                }
            }
            DriveMode::Latching => {
                let mut open = output(config.open_pins[i])?;
                let mut close = output(config.close_pins[i])?;
                open.set_low()?;
                close.set_low()?;
                Actuator::Latching {
                    open,
                    close,
                    pulse_ms: config.pulse_ms,
                }
            }
        };
        actuators.push(actuator);
    }

    // Latching solenoids power up in an unknown position; pulse them all closed
    // so our tracked state (off) matches the physical valve. Fire sequentially
    // with a settle gap between valves so inrush currents don't stack.
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
    /// Drive this valve toward `want`, updating the caller's tracked `actual`.
    pub fn apply(&mut self, want: bool, actual: &mut bool) -> anyhow::Result<()> {
        match self {
            Actuator::Ac { pin, active_low } => {
                // active_low relay boards energize on a low output.
                if want ^ *active_low {
                    pin.set_high()?;
                } else {
                    pin.set_low()?;
                }
                *actual = want;
            }
            Actuator::Latching {
                open,
                close,
                pulse_ms,
            } => {
                if want != *actual {
                    let line = if want { open } else { close };
                    line.set_high()?;
                    FreeRtos::delay_ms(*pulse_ms);
                    line.set_low()?;
                    *actual = want;
                }
            }
        }
        Ok(())
    }
}
