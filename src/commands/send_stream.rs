use crate::action::{Action, Processing};
use crate::commands::{Command};
use crate::state::{State};
use crate::message::{NetMessage};
use crate::util::{Result, Reportable};

use message_io::network::{Network};
use v4l::prelude::*;
use v4l::FourCC;

// Send Stream logic

pub struct SendStreamCommand;
impl Command for SendStreamCommand {
    fn name(&self) -> &'static str {
        "stream"
    }

    fn parse_params(&self, _params: &[&str]) -> Result<Box<dyn Action>> {
        match SendStream::new() {
            Ok(action) => Ok(Box::new(action)),
            Err(e) => Err(e),
        }
    }
}
pub struct SendStream {
    stream: MmapStream<'static>,
    width: usize,
    height: usize,
}

impl SendStream {
    pub fn new() -> Result<SendStream> {
        let mut dev = CaptureDevice::new(0).expect("Failed to open device");

        let mut fmt = dev.format()?;
        fmt.fourcc = FourCC::new(b"YUYV");
        let width = fmt.width as usize;
        let height = fmt.height as usize;
        dev.set_format(&fmt)?;

        let stream = MmapStream::with_buffers(&dev, 4)?;

        Ok(SendStream { stream, width, height })
    }
}

impl Action for SendStream {
    fn process(&mut self, mut state: &mut State, network: &mut Network) -> Processing {
        if state.stop_stream {
            // stop stream and restore stop_stream to false for the next stream usage
            state.stop_stream = false;
            network.send_all(state.all_user_endpoints(), NetMessage::Stream(None));
            return Processing::Completed
        }
        let data = match self.stream.next() {
            Ok(d) => d,
            Err(e) => {
                e.to_string().report_err(&mut state);
                network.send_all(state.all_user_endpoints(), NetMessage::Stream(None));
                return Processing::Completed
            }
        };
        let data = data
            .data()
            .chunks_exact(4)
            .map(|v| {
                //safe unwrap due to chunks 4 making sure its a [u8;4]
                let v = crate::util::yuyv_to_rgb(std::convert::TryFrom::try_from(v).unwrap());
                u32::from_be_bytes(v)
            })
            .collect();

        let message = NetMessage::Stream(Some((data, self.width, self.height)));
        network.send_all(state.all_user_endpoints(), message);
        Processing::Partial
    }
}

// Stop stream logic

pub struct StopStreamCommand;

impl Command for StopStreamCommand {
    fn name(&self) -> &'static str {
        "stopstream"
    }

    fn parse_params(&self, _params: &[&str]) -> Result<Box<dyn Action>> {
        Ok(Box::new(StopStream {}))
    }
}
struct StopStream {}
impl Action for StopStream {
    fn process(&mut self, state: &mut State, _network: &mut Network) -> Processing {
        state.stop_stream = true;
        Processing::Completed
    }
}
