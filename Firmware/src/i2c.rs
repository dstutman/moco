// TODO: Test and correct if issues
// Currently only designed for ESP32C3 with single I2C peripheral.
use core::sync::atomic::{AtomicBool, Ordering};

use esp32c3_hal::pac::{self, interrupt};

static INTERRUPT_TRIGGERED: AtomicBool = AtomicBool::new(false);

interrupt!(I2C_EXT0, i2c0_isr);
pub fn i2c0_isr() {
    INTERRUPT_TRIGGERED.store(true, Ordering::SeqCst);
}

#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    BusError,
}

pub enum BaudRate {
    Standard,
    Fast,
}

pub struct Master {
    regs: pac::I2C0,
}

impl Master {
    /// Creates a new `Master` control instance. Assumes desired pin
    /// configuration is already applied.
    pub fn new(regs: pac::I2C0) -> Self {
        // Enable the external crystal oscillator as the clock source
        regs.clk_conf
            .write(|w| w.sclk_sel().clear_bit().sclk_active().set_bit());
        // Configure the peripheral as a master
        regs.ctr.write(|w| w.ms_mode().set_bit());
        // Synchronize clock domains
        regs.ctr.modify(|_, w| unsafe { w.conf_upgate().set_bit() });
        Self { regs }
    }

    pub fn set_baudrate(&mut self, baud: BaudRate) {
        match baud {
            BaudRate::Standard => self.regs.clk_conf.modify(|_, w| unsafe {
                w.sclk_div_num()
                    .bits(100)
                    .sclk_div_b()
                    .bits(u8::MAX)
                    .sclk_div_a()
                    .bits(0)
            }),
            BaudRate::Fast => self.regs.clk_conf.modify(|_, w| unsafe {
                w.sclk_div_num()
                    .bits(40)
                    .sclk_div_b()
                    .bits(u8::MAX)
                    .sclk_div_a()
                    .bits(0)
            }),
        }
    }

    pub fn read(&mut self, device_id: u8, address: u8, data: &[u8]) -> Result<(), Error> {
        assert!(data.len() <= 32);
        // Transaction configuration
        // Command 0: START
        self.regs.comd[0].write(|w| unsafe { w.command().bits((0 << 11) | (1 << 8)) });
        // Command 1: WRITE
        // `data.len()` is u32, but high bits are guaranteed by
        self.regs.comd[1].write(|w| unsafe { w.command().bits((1 << 11) | (1 << 8) | 1) });
        // Command 2: RESTART
        self.regs.comd[2].write(|w| unsafe { w.command().bits((0 << 11) | (1 << 8)) });
        // Command 3: READ
        // `data.len()` is u32, but high bits are guaranteed unset by assert
        self.regs.comd[3]
            .write(|w| unsafe { w.command().bits((1 << 11) | (1 << 8) | data.len() as u16) });
        // Command 4: STOP
        self.regs.comd[4].write(|w| unsafe { w.command().bits((3 << 11) | (1 << 8)) });

        // Configure the slave address
        self.regs
            .slave_addr
            .write(|w| unsafe { w.slave_addr().bits(device_id as u16) });

        // Write data into the TX buffer in FIFO mode
        self.regs
            .data
            .write(|w| unsafe { w.fifo_rdata().bits(address) });

        // Synchronize clock domains
        self.regs
            .ctr
            .modify(|_, w| unsafe { w.conf_upgate().set_bit() });

        // Start the transaction
        self.regs.ctr.modify(|_, w| w.trans_start().set_bit());

        // Wait for transaction completion, then reset flag
        while !INTERRUPT_TRIGGERED
            .compare_exchange(true, false, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {}
        let int_status = self.regs.int_status.read();
        // Reset the interrupt flags
        self.regs.int_clr.write(|w| unsafe { w.bits(u32::MAX) });

        // If we were triggered by a completed transaction, OK, else error
        if int_status.trans_complete_int_st().bit_is_set() {
            Ok(())
        } else {
            Err(Error::BusError)
        }
    }

    pub fn write(&mut self, device_id: u8, address: u8, data: &[u8]) -> Result<(), Error> {
        assert!(data.len() <= 32);
        // Transaction configuration
        // Command 0: START
        self.regs.comd[0].write(|w| unsafe { w.command().bits((0 << 11) | (1 << 8)) });
        // Command 1: WRITE
        // `data.len()` is u32, but high bits are guaranteed unset by assert
        self.regs.comd[1].write(|w| unsafe {
            w.command()
                .bits((1 << 11) | (1 << 8) | (data.len() + 1) as u16)
        });
        // Command 2: STOP
        self.regs.comd[2].write(|w| unsafe { w.command().bits((3 << 11) | (1 << 8)) });

        // Configure the slave address
        self.regs
            .slave_addr
            .write(|w| unsafe { w.slave_addr().bits(device_id as u16) });

        // Write data into the TX buffer in FIFO mode
        self.regs
            .data
            .write(|w| unsafe { w.fifo_rdata().bits(address) });
        for byte in data {
            self.regs
                .data
                .write(|w| unsafe { w.fifo_rdata().bits(*byte) })
        }

        // Synchronize clock domains
        self.regs
            .ctr
            .modify(|_, w| unsafe { w.conf_upgate().set_bit() });

        // Start the transaction
        self.regs.ctr.modify(|_, w| w.trans_start().set_bit());

        // Wait for transaction completion, then reset flag
        while !INTERRUPT_TRIGGERED
            .compare_exchange(true, false, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {}
        let int_status = self.regs.int_status.read();
        // Reset the interrupt flags
        self.regs.int_clr.write(|w| unsafe { w.bits(u32::MAX) });

        // If we were triggered by a completed transaction, OK, else error
        if int_status.trans_complete_int_st().bit_is_set() {
            Ok(())
        } else {
            Err(Error::BusError)
        }
    }
}
