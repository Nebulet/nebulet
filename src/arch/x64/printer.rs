use core::fmt::{Write, Result};
use core::ptr;

use arch::lock::Spinlock;

const VGA_BUFFER: *mut u16 = 0xb8000 as *mut _;
const SCREEN_SIZE: usize = 80 * 25;

pub static PRINTER: Spinlock<Printer> = Spinlock::new(Printer::new());

pub struct Printer {
    index: usize,
}

impl Printer {
    pub const fn new() -> Printer {
        Printer {
            index: 0,
        }
    }

    #[inline]
    pub fn write_byte(&mut self, byte: u8) {
        match byte {
            b'\n' => self.newline(),
            _ => {
                let character = byte as u16 | 0xf << 8;
                unsafe {
                    ptr::write_volatile(VGA_BUFFER.add(self.index), character);
                }
                self.index += 1;
            }
        }
        if self.index >= SCREEN_SIZE {
            self.index = 0;
        }
    }

    fn newline(&mut self) {
        self.index += 80 - (self.index % 80);
    }
}

impl Write for Printer {
    #[inline]
    fn write_str(&mut self, s: &str) -> Result {
        for byte in s.bytes() {
            self.write_byte(byte);
        }

        Ok(())
    }
}

pub fn _print(args: ::core::fmt::Arguments) {
    let _ = PRINTER.lock().write_fmt(args);
}
