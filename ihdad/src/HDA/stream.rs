use std::{mem, thread, ptr, fmt};
use std::cmp::max;

use syscall::MAP_WRITE;
use syscall::error::{Error, EACCES, EWOULDBLOCK, Result};
use syscall::flag::O_NONBLOCK;
use syscall::io::{Dma, Mmio, Io, ReadOnly};
use syscall::scheme::SchemeMut;
use std::sync::Arc;
use std::cell::RefCell;
use std::result;


extern crate syscall;

pub enum BaseRate {
	BR44_1,
	BR48,
}


pub struct SampleRate {
	base: BaseRate,
	mult: u16,
	div:  u16,
}

use self::BaseRate::{BR44_1, BR48};

pub const SR_8:      SampleRate = SampleRate {base: BR48  , mult:  1, div:  6};
pub const SR_11_025: SampleRate = SampleRate {base: BR44_1, mult:  1, div:  4};
pub const SR_16:     SampleRate = SampleRate {base: BR48  , mult:  1, div:  3};
pub const SR_22_05:  SampleRate = SampleRate {base: BR44_1, mult:  1, div:  2};
pub const SR_32:     SampleRate = SampleRate {base: BR48  , mult:  2, div:  3};

pub const SR_44_1:   SampleRate = SampleRate {base: BR44_1, mult:  1, div:  1};
pub const SR_48:     SampleRate = SampleRate {base: BR48  , mult:  1, div:  1};
pub const SR_88_1:   SampleRate = SampleRate {base: BR44_1, mult:  2, div:  1};
pub const SR_96:     SampleRate = SampleRate {base: BR48  , mult:  2, div:  1};
pub const SR_176_4:  SampleRate = SampleRate {base: BR44_1, mult:  4, div:  1};
pub const SR_192:    SampleRate = SampleRate {base: BR48  , mult:  4, div:  1};





#[repr(u8)]
pub enum BitsPerSample {
	Bits8  = 0,
	Bits16 = 1,
	Bits20 = 2,
	Bits24 = 3,
	Bits32 = 4,	
}



pub fn format_to_u16(sr: &SampleRate, bps: BitsPerSample, channels:u8) -> u16{
	

	// 3.3.41
	
	let base:u16 = match sr.base {
		BaseRate::BR44_1 => { 1 << 14},
		BaseRate::BR48   => { 0 },
	};

	let mult = ((sr.mult - 1) & 0x7) << 11;

	let div  = ((sr.div  - 1) & 0x7) <<  8;

	let bits = (bps as u16) << 4;

	let chan = ((channels - 1) & 0xF) as u16;

	let val:u16 = base | mult | div | bits | chan;
	
	val

}


#[repr(packed)]
pub struct StreamDescriptorRegs {
	ctrl_lo:            Mmio<u16>,
	ctrl_hi:            Mmio<u8>,
	status:             Mmio<u8>,
	link_pos:           Mmio<u32>,
	buff_length:        Mmio<u32>,
	last_valid_index:   Mmio<u16>,
	resv1:              Mmio<u16>,
	fifo_size_:         Mmio<u16>,
	format:             Mmio<u16>,
	resv2:              Mmio<u32>,
	buff_desc_list_lo:  Mmio<u32>,
	buff_desc_list_hi:  Mmio<u32>, 

}


impl StreamDescriptorRegs {
	pub fn status(&self) -> u8 {
		self.status.read()
	}

	pub fn set_status(&mut self, status: u8){
		self.status.write(status);
	}

	pub fn control(&self) -> u32 {
		let mut ctrl = self.ctrl_lo.read() as u32;
		ctrl |= (self.ctrl_hi.read() as u32) << 16;
		ctrl	
	}

	pub fn set_control(&mut self, control:u32) {
		self.ctrl_lo.write((control & 0xFFFF) as u16);
		self.ctrl_hi.write(((control >> 16) & 0xFF) as u8);
	}

	pub fn set_pcm_format(&mut self, sr: &SampleRate, bps: BitsPerSample, channels:u8) {
		

		// 3.3.41
		
		let val = format_to_u16(sr,bps,channels);
		self.format.write(val);

	}

	pub fn fifo_size(&self) -> u16 {
		self.fifo_size_.read()
	}

	pub fn set_cyclic_buffer_length(&mut self, length: u32) {
		self.buff_length.write(length);
	}

	pub fn cyclic_buffer_length(&self) -> u32 {
		self.buff_length.read()
	}

	pub fn run(&mut self) {
		let val = self.control() | (1 << 1);
		self.set_control(val);
	}

	pub fn stop(&mut self) {
		let val = self.control() & !(1 << 1);
		self.set_control(val);
	}
	
	pub fn stream_number(&self) -> u8 {
		((self.control() >> 20) & 0xF) as u8
	}
	
	pub fn set_stream_number(&mut self, stream_number: u8) {
		let val = (self.control() & 0x00FFFF ) | (((stream_number & 0xF ) as u32) << 20);
		self.set_control(val);
	}

	pub fn set_address(&mut self, addr: usize) {
		self.buff_desc_list_lo.write( (addr & 0xFFFFFFFF) as u32);
		self.buff_desc_list_hi.write( ( (addr >> 32) & 0xFFFFFFFF) as u32);
	}

	pub fn set_last_valid_index(&mut self, index:u16) {
		self.last_valid_index.write(index);
	}

	pub fn link_position(&self) -> u32 {
		self.link_pos.read()
	}

	pub fn set_interrupt_on_completion(&mut self, enable:bool) {
		let mut ctrl = self.control();
		if enable {
			ctrl |= (1 << 2);
		} else {
			ctrl &= !(1 << 2);
		}
		self.set_control(ctrl);
	}

	pub fn buffer_complete(&self) -> bool {
		self.status.readf(1 << 2)
	}

	pub fn clear_interrupts(&mut self) {
		self.status.write(0x7 << 2);
	}

	// get sample size in bytes
	pub fn sample_size(&self) -> usize {
		let format = self.format.read();	
		let chan = (format & 0xF) as usize;
		let bits = ((format >> 4) & 0xF) as usize;
		match bits {
			0 => 1 * (chan + 1),
			1 => 2 * (chan + 1),
			_ => 4 * (chan + 1),
		}
	}
}

pub struct Stream {
	buff:                   usize,
	buff_phys:              usize,
	buff_length:            usize,
	output_current_block:   usize,
	block_length:           usize,
	block_count:            usize,
}


#[repr(packed)]
pub struct BufferDescriptorListEntry {
	addr:     Mmio<u64>,
	len:      Mmio<u32>,
	ioc_resv: Mmio<u32>,
}

impl BufferDescriptorListEntry {
	pub fn address(&self) -> usize {
		self.addr.read() as usize
	}

	pub fn set_address(&mut self, addr:usize) {
		self.addr.write(addr as u64);
	}

	pub fn length(&self) -> u32 {
		self.len.read()
	}

	pub fn set_length(&mut self, length: u32) {
		self.len.write(length)
	}

	pub fn interrupt_on_completion(&self) -> bool {
		(self.ioc_resv.read() & 0x1) == 0x1
	}

	pub fn set_interrupt_on_complete(&mut self, ioc: bool) {
		self.ioc_resv.writef(1, ioc);
	}
}



pub struct StreamBuffer {
	phys:   usize,
	addr:   usize,
	
	len: usize,
}

impl StreamBuffer {
	pub fn new(length: usize) -> result::Result<StreamBuffer, &'static str> {
		let phys = unsafe {
			syscall::physalloc(length)
		};
		if !phys.is_ok() {
			return Err("Could not allocate physical memory for buffer.");
		}

		let phys_addr = phys.unwrap();

		let addr = unsafe { 
			syscall::physmap(phys_addr, length, MAP_WRITE)
		};

		if !addr.is_ok() {
			return Err("Could not map physical memory for buffer.");
		} else {
			unsafe {syscall::physfree(phys_addr, length);}
		}
		
		Ok(StreamBuffer {
			phys:   phys_addr,
			addr:   addr.unwrap(),
			len: length,
		})
	}

	pub fn length(&self) -> usize {
		self.len
	}
	
	pub fn addr(&self) -> usize {
		self.addr
	}

	pub fn phys(&self) -> usize {
		self.phys
	}

}
/*
impl Drop for StreamBuffer {
	fn drop(&mut self) {
		unsafe {
			print!("IHDA: Deallocating buffer.\n");
			syscall::physunmap(self.addr);
			syscall::physfree(self.phys, self.len);
		}
	}
}*/
