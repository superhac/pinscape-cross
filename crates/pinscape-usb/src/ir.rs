//! IR: VendorClient methods for CMD_QUERY_IR (0x14), CMD_SEND_IR (0x0D), CMD_TVON (0x0A): received IR commands and raw pulses, sending IR, TV-ON state and relay. (Phase 3)
//!
//! Mirrors QueryIRCommandsReceived, QueryIRRawPulsesReceived, SendIRCommand,
//! QueryTVONState, SetTVRelayManualState and PulseTVRelay in the upstream
//! WinAPI/PinscapeVendorInterface.cpp.
//!
//! Firmware behavior worth knowing: with no `irRx` configured, the receive
//! queries simply return empty lists; with no `irTx`, SEND_IR fails with
//! `ERR_FAILED` (as it also does when the transmit queue is full). TV ON
//! state is reported whether or not `tvon` is configured.

use pinscape_protocol::ir::{
    IrCode, IrCommandReceived, IrRawPulse, SendIrArgs, TvOnState, TvRelayAction, SUBCMD_QUERY_IR_CMD,
    SUBCMD_QUERY_IR_RAW, SUBCMD_TVON_QUERY_STATE,
};
use pinscape_protocol::{cmd, Status};

use crate::{Error, Result, VendorClient};

impl VendorClient {
    /// Decoded IR commands received since the last call (the device clears its buffer).
    pub fn query_ir_commands(&mut self) -> Result<Vec<IrCommandReceived>> {
        let (_, data) = self.transact(cmd::QUERY_IR, &[SUBCMD_QUERY_IR_CMD], &[])?;
        Ok(IrCommandReceived::decode_list(&data)?)
    }

    /// Raw IR pulses received since the last call. The device starts a new
    /// buffer at each code (after a quiet period of 130 ms or more) and the
    /// query clears it, so a caller polling during a transmission sees the
    /// pulse train in consecutive pieces.
    pub fn query_ir_raw(&mut self) -> Result<Vec<IrRawPulse>> {
        let (_, data) = self.transact(cmd::QUERY_IR, &[SUBCMD_QUERY_IR_RAW], &[])?;
        Ok(IrRawPulse::decode_list(&data)?)
    }

    /// Queue an IR command for transmission `count` times (1..=255).
    pub fn send_ir(&mut self, code: IrCode, count: u8) -> Result<()> {
        if count == 0 {
            return Err(Error::Status { code: Status::OutOfBounds as u16 });
        }
        let args = SendIrArgs { code, count }.encode();
        self.transact(cmd::SEND_IR, &args, &[]).map(|_| ())
    }

    /// Current TV ON state machine, power-sense and relay status.
    pub fn query_tvon_state(&mut self) -> Result<TvOnState> {
        let (r, _) = self.transact(cmd::TVON, &[SUBCMD_TVON_QUERY_STATE], &[])?;
        Ok(TvOnState::decode(&r)?)
    }

    /// Switch the TV relay manually on or off, or pulse it for the configured time.
    pub fn set_tv_relay(&mut self, action: TvRelayAction) -> Result<()> {
        self.transact(cmd::TVON, &action.encode(), &[]).map(|_| ())
    }
}
