//! MIDI devices through midir: every input plays into the engine, and the
//! band can go out to one output, each message sent when it is heard.

use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;
use std::time::Instant;

use midir::{MidiInput, MidiInputConnection, MidiOutput, MidiOutputConnection};
use midzer_core::{Command, DRUM_CHANNEL, EngineEvent, WORDS_PER_EVENT};

use crate::Shared;

const CLIENT: &str = "midzer";
/// Band channels silenced on stop; mirrors the parts in style.rs.
const BAND_CHANNELS: [u8; 4] = [1, 2, 3, DRUM_CHANNEL];

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Port {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ports {
    pub inputs: Vec<Port>,
    pub outputs: Vec<Port>,
}

pub(crate) struct Midi {
    shared: Arc<Shared>,
    inputs: BTreeMap<String, (String, MidiInputConnection<()>)>,
    outputs: Vec<Port>,
    output: Option<(String, MidiOutputConnection)>,
    /// Band messages waiting for the moment they are heard.
    pending: VecDeque<(Instant, [u8; 3])>,
    reported: Option<Ports>,
}

impl Midi {
    pub(crate) fn new(shared: Arc<Shared>) -> Self {
        Self { shared, inputs: BTreeMap::new(), outputs: Vec::new(), output: None, pending: VecDeque::new(), reported: None }
    }

    pub(crate) fn ports(&self) -> Ports {
        Ports {
            inputs: self.inputs.iter().map(|(id, (name, _))| Port { id: id.clone(), name: name.clone() }).collect(),
            outputs: self.outputs.clone(),
        }
    }

    /// Connects new inputs, drops vanished ones and lists outputs. Returns the
    /// ports if they changed since the last call.
    pub(crate) fn rescan(&mut self) -> Option<Ports> {
        if let Ok(probe) = MidiInput::new(CLIENT) {
            let present: Vec<(String, String)> = probe
                .ports()
                .iter()
                .filter_map(|p| Some((p.id(), probe.port_name(p).ok()?)))
                .filter(|(_, name)| !name.contains("Midi Through") && !name.starts_with(CLIENT))
                .collect();
            self.inputs.retain(|id, _| present.iter().any(|(p, _)| p == id));
            for (id, name) in present {
                if !self.inputs.contains_key(&id)
                    && let Some(connection) = self.connect_input(&id)
                {
                    self.inputs.insert(id, (name, connection));
                }
            }
        }
        if let Ok(probe) = MidiOutput::new(CLIENT) {
            self.outputs = probe
                .ports()
                .iter()
                .filter_map(|p| Some(Port { id: p.id(), name: probe.port_name(p).ok()? }))
                .filter(|p| !p.name.contains("Midi Through") && !p.name.starts_with(CLIENT))
                .collect();
            if let Some((id, _)) = &self.output
                && !self.outputs.iter().any(|p| &p.id == id)
            {
                self.output = None;
            }
        }
        let ports = self.ports();
        (self.reported.as_ref() != Some(&ports)).then(|| {
            self.reported = Some(ports.clone());
            ports
        })
    }

    fn connect_input(&self, id: &str) -> Option<MidiInputConnection<()>> {
        let input = MidiInput::new(CLIENT).ok()?;
        let port = input.find_port_by_id(id)?;
        let shared = self.shared.clone();
        input
            .connect(
                &port,
                "midzer-in",
                move |_, bytes, _| {
                    if let Some(cmd) = Command::from_midi(bytes) {
                        let words = cmd.encode();
                        let _ = shared.commands.push(words);
                        let _ = shared.inputs.push(words);
                    }
                },
                (),
            )
            .ok()
    }

    pub(crate) fn set_output(&mut self, id: Option<&str>) -> Result<(), String> {
        self.silence();
        self.output = None;
        self.pending.clear();
        let Some(id) = id else { return Ok(()) };
        let out = MidiOutput::new(CLIENT).map_err(|e| e.to_string())?;
        let port = out.find_port_by_id(id).ok_or("That MIDI output is gone.")?;
        let connection = out.connect(&port, "midzer-out").map_err(|e| e.to_string())?;
        self.output = Some((id.to_string(), connection));
        Ok(())
    }

    /// Queues the band's MIDI from encoded engine events, each at the time it is heard.
    pub(crate) fn schedule(&mut self, words: &[u32]) {
        if self.output.is_none() {
            return;
        }
        let heard = self.shared.heard_frame() as u64;
        for record in words.as_chunks::<WORDS_PER_EVENT>().0 {
            let Some((event, frame)) = EngineEvent::decode([record[0], record[1], record[2], record[3]]) else {
                continue;
            };
            // Frames travel as u32; unwrap against the frame being heard.
            let wraps = (heard as i64 - frame as i64 + (1 << 31)) >> 32;
            let frame = (frame as i64 + (wraps << 32)).max(0) as u64;
            let at = self.shared.instant_of(frame);
            if let Some(bytes) = event.to_midi() {
                self.pending.push_back((at, bytes));
            } else if event == (EngineEvent::Transport { playing: false }) {
                for channel in BAND_CHANNELS {
                    self.pending.push_back((at, [0xb0 | channel, 123, 0]));
                }
            }
        }
    }

    /// Sends every queued message whose time has come.
    pub(crate) fn flush(&mut self) {
        let now = Instant::now();
        let Some((_, out)) = &mut self.output else { return };
        while let Some((at, bytes)) = self.pending.front() {
            if *at > now {
                break;
            }
            let _ = out.send(&bytes[..message_len(bytes[0])]);
            self.pending.pop_front();
        }
    }

    fn silence(&mut self) {
        if let Some((_, out)) = &mut self.output {
            for channel in BAND_CHANNELS {
                let _ = out.send(&[0xb0 | channel, 123, 0]);
            }
        }
    }
}

fn message_len(status: u8) -> usize {
    match status & 0xf0 {
        0xc0 | 0xd0 => 2,
        _ => 3,
    }
}
