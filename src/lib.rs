#![allow(
  non_camel_case_types,
  non_upper_case_globals,
  non_snake_case,
  private_interfaces,
  dead_code
)]
#![feature(once_cell_get_mut)]

use {
  critical_section::Mutex,
  std::{cell::UnsafeCell, ffi::c_char, str}
};

pub type char8_t = u8;
pub type char16_t = u16;
pub type char32_t = u32;
pub type size_t = usize;
pub type ssize_t = isize;

#[repr(C)]
#[derive(Clone, Copy)]
struct MBState {
  count: usize,
  u8_buffer: [char8_t; 4],
  u8_position: usize,
  u16_buffer: [char16_t; 2],
  u16_surrogate: char16_t
}

impl MBState {
  pub const fn new() -> Self {
    Self {
      count: 0,
      u8_buffer: [0; 4],
      u8_position: 0,
      u16_buffer: [0; 2],
      u16_surrogate: 0
    }
  }

  pub fn is_initial(&self) -> bool {
    self.count == 0 &&
      self.u8_position == 0 &&
      (self.u16_surrogate < 0xd800 || self.u16_surrogate > 0xdfff)
  }

  pub fn reset(&mut self) {
    self.count = 0;
    self.u8_buffer = [0; 4];
    self.u8_position = 0;
  }
}

pub type mbstate_t = MBState;

pub const MB_LEN_MAX: usize = 16;

#[unsafe(no_mangle)]
pub extern "C" fn rs_c8rtomb(
  s: *mut c_char,
  c8: char8_t,
  ps: *mut mbstate_t
) -> size_t {
  static GLOBAL: Mutex<UnsafeCell<MBState>> =
    Mutex::new(UnsafeCell::new(MBState::new()));
  let ps: &mut MBState = if !ps.is_null() {
    unsafe { &mut *ps }
  } else {
    critical_section::with(|cs| {
      let cell = GLOBAL.borrow(cs);
      unsafe { &mut *cell.get() }
    })
  };

  let mut buf: [c_char; MB_LEN_MAX as usize] = [0; MB_LEN_MAX as usize];
  let (s, c8) = if s.is_null() { (buf.as_mut_ptr(), 0) } else { (s, c8) };

  if ps.u8_position == 0 {
    if (c8 >= 0x80 && c8 <= 0xc1) || c8 >= 0xf5 {
      // EILSEQ
      return -1isize as size_t;
    }
    if c8 >= 0xc2 {
      ps.u8_position = 1;
      ps.u8_buffer[0] = c8;
      return 0;
    }

    ps.reset();
    c32tomb(s, c8 as char32_t, ps) as size_t
  } else {
    if ps.u8_position == 1 {
      if (c8 < 0x80 || c8 > 0xbf) ||
        (ps.u8_buffer[0] == 0xe0 && c8 < 0xa0) ||
        (ps.u8_buffer[0] == 0xed && c8 > 0x9f) ||
        (ps.u8_buffer[0] == 0xf0 && c8 < 0x90) ||
        (ps.u8_buffer[0] == 0xf4 && c8 > 0xbf)
      {
        // EILSEQ
        return -1isize as size_t;
      }

      if ps.u8_buffer[0] >= 0xe0 {
        ps.u8_buffer[ps.u8_position] = c8;
        ps.u8_position += 1;
        return 0;
      }
    } else {
      if c8 < 0x80 || c8 > 0xbf {
        // EILSEQ
        return -1isize as size_t;
      }

      if ps.u8_position == 2 && ps.u8_buffer[0] >= 0xf0 {
        ps.u8_buffer[ps.u8_position] = c8;
        ps.u8_position += 1;
        return 0;
      }
    }

    ps.u8_buffer[ps.u8_position] = c8;
    ps.u8_position += 1;

    match str::from_utf8(&ps.u8_buffer[..ps.u8_position]) {
      | Ok(decoded) => {
        if let Some(c32) = decoded.chars().next() {
          ps.reset();
          return c32tomb(s, c32 as char32_t, ps) as size_t;
        }
        decoded.len()
      },
      | Err(_) => {
        // EILSEQ
        -1isize as size_t
      }
    }
  }
}

#[unsafe(no_mangle)]
pub extern "C" fn rs_c16rtomb(
  s: *mut c_char,
  c16: char16_t,
  ps: *mut mbstate_t
) -> size_t {
  static GLOBAL: Mutex<UnsafeCell<MBState>> =
    Mutex::new(UnsafeCell::new(MBState::new()));
  let ps: &mut MBState = if !ps.is_null() {
    unsafe { &mut *ps }
  } else {
    critical_section::with(|cs| {
      let cell = GLOBAL.borrow(cs);
      unsafe { &mut *cell.get() }
    })
  };

  let mut buf: [c_char; MB_LEN_MAX as usize] = [0; MB_LEN_MAX as usize];
  let (s, c16) = if s.is_null() { (buf.as_mut_ptr(), 0) } else { (s, c16) };

  if !ps.is_initial() {
    let units = [ps.u16_surrogate, c16];
    let mut decoder = char::decode_utf16(units.iter().copied());

    match decoder.next() {
      | Some(Ok(c)) => {
        if decoder.next().is_some() {
          return -1isize as size_t;
        }
        ps.reset();
        c32tomb(s, c as char32_t, ps) as size_t
      },
      | _ => -1isize as size_t
    }
  } else {
    let units = [c16];
    let mut decoder = char::decode_utf16(units.iter().copied());

    if let Some(next) = decoder.next() {
      match next {
        | Ok(c) => {
          ps.reset();
          c32tomb(s, c as char32_t, ps) as size_t
        },
        | Err(e) => {
          if (0xD800..=0xDBFF).contains(&e.unpaired_surrogate()) {
            ps.u16_surrogate = e.unpaired_surrogate();
            return 0;
          }
          {
            // EILSEQ
            -1isize as size_t
          }
        }
      }
    } else {
      // EILSEQ
      -1isize as size_t
    }
  }
}

#[unsafe(no_mangle)]
pub extern "C" fn rs_c32rtomb(
  s: *mut c_char,
  c32: char32_t,
  ps: *mut mbstate_t
) -> size_t {
  static GLOBAL: Mutex<UnsafeCell<MBState>> =
    Mutex::new(UnsafeCell::new(MBState::new()));
  let ps: &mut MBState = if !ps.is_null() {
    unsafe { &mut *ps }
  } else {
    critical_section::with(|cs| {
      let cell = GLOBAL.borrow(cs);
      unsafe { &mut *cell.get() }
    })
  };

  let mut buf: [c_char; MB_LEN_MAX as usize] = [0; MB_LEN_MAX as usize];
  let (s, c32) = if s.is_null() { (buf.as_mut_ptr(), 0) } else { (s, c32) };

  ps.reset();
  c32tomb(s, c32, ps) as size_t
}

#[unsafe(no_mangle)]
pub extern "C" fn rs_mbrtoc32(
  pc32: *mut char32_t,
  s: *const c_char,
  n: size_t,
  ps: *mut mbstate_t
) -> size_t {
  static GLOBAL: Mutex<UnsafeCell<MBState>> =
    Mutex::new(UnsafeCell::new(MBState::new()));
  let ps: &mut MBState = if !ps.is_null() {
    unsafe { &mut *ps }
  } else {
    critical_section::with(|cs| {
      let cell = GLOBAL.borrow(cs);
      unsafe { &mut *cell.get() }
    })
  };

  let (pc32, s, n) = if s.is_null() {
    (0 as *mut char32_t, 0 as *const c_char, 1 as size_t)
  } else {
    (pc32, s, n)
  };

  let l: ssize_t = mbtoc32(pc32, s, n, ps);
  if l >= 0 && unsafe { *pc32 == '\0' as char32_t } {
    return 0;
  }
  l as size_t
}

// BOILERPLATE
fn c32tomb(
  s: *mut c_char,
  c32: char32_t,
  ps: &mut MBState
) -> ssize_t {
  if s.is_null() {
    return 0;
  }

  // Convert char32_t to Rust char
  let rust_char = match char::from_u32(c32) {
    | Some(c) => c,
    | None => return -1 // Invalid Unicode code point
  };

  // Convert to UTF-8 bytes
  let mut utf8_buf = [0u8; 4];
  let utf8_str = rust_char.encode_utf8(&mut utf8_buf);
  let utf8_bytes = utf8_str.as_bytes();

  // Copy UTF-8 bytes to output buffer
  unsafe {
    for (i, &byte) in utf8_bytes.iter().enumerate() {
      *s.add(i) = byte as c_char;
    }
  }

  // Reset state after successful conversion
  ps.count = 0;
  ps.u8_buffer = [0; 4];

  utf8_bytes.len() as ssize_t
}

fn mbtoc32(
  _pc32: *mut char32_t,
  _s: *const c_char,
  _n: size_t,
  _ps: &mut MBState
) -> ssize_t {
  todo!()
}
