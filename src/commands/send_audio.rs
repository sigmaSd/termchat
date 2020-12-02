use crate::{
    action::{Action, Processing},
    message::{Chunk, NetMessage},
};
use crate::commands::{Command};
use crate::state::{State};
use crate::util::{Result};

use message_io::network::{Network};

pub struct SendAudioCommand;

impl Command for SendAudioCommand {
    fn name(&self) -> &'static str {
        "sa"
    }

    fn parse_params(&self, _params: &[&str]) -> Result<Box<dyn Action>> {
        Ok(Box::new(SendAudio::new().unwrap()))
    }
}

pub struct SendAudio {
    audio: std::process::Child,
}

impl SendAudio {
    pub fn new() -> Result<SendAudio> {
        let audio = std::process::Command::new("arecord")
            .args(&["-f", "dat"])
            .stderr(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .spawn()?;

        Ok(SendAudio { audio })
    }
}

use std::io::Read;
impl Action for SendAudio {
    fn process(&mut self, state: &mut State, network: &mut Network) -> Processing {
        let mut chunk = vec![0; 33000];
        let n = self.audio.stdout.as_mut().unwrap().read(&mut chunk).unwrap();

        let message = NetMessage::UserData("AUDIO".into(), Chunk::Stream(chunk[..n].to_vec()));

        network.send_all(state.all_user_endpoints(), message);
        Processing::Partial
    }
}
