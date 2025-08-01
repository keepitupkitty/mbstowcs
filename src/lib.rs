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

pub type c_int = i32;
pub type char8_t = u8;
pub type char16_t = u16;
pub type char32_t = u32;
pub type size_t = usize;
pub type ssize_t = isize;

#[repr(C)]
#[derive(Clone, Copy)]
struct MBState {
  ch: char32_t,
  bytesleft: usize,
  partial: char32_t,
  lowerbound: char32_t,
  u8_buffer: [char8_t; 4],
  u8_position: usize,
  u16_buffer: [char16_t; 2],
  u16_surrogate: char16_t
}

impl MBState {
  pub const fn new() -> Self {
    Self {
      ch: 0,
      bytesleft: 0,
      partial: 0,
      lowerbound: 0,
      u8_buffer: [0; 4],
      u8_position: 0,
      u16_buffer: [0; 2],
      u16_surrogate: 0
    }
  }

  pub fn is_initial(&self) -> bool {
    self.ch == 0 &&
      self.bytesleft == 0 &&
      (self.u16_surrogate < 0xd800 || self.u16_surrogate > 0xdfff)
  }

  pub fn reset(&mut self) {
    self.ch = 0;
    self.bytesleft = 0;
    self.partial = 0;
    self.lowerbound = 0;
    self.u8_buffer = [0; 4];
    self.u8_position = 0;
    self.u16_buffer = [0; 2];
    self.u16_surrogate = 0;
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
      //errno::set_errno(errno::EILSEQ);
      return -1isize as size_t;
    }
    if c8 >= 0xc2 {
      ps.u8_position = 1;
      ps.u8_buffer[0] = c8;
      return 0;
    }

    ps.reset();
    c32tomb(s, c8 as char32_t) as size_t
  } else {
    if ps.u8_position == 1 {
      if (c8 < 0x80 || c8 > 0xbf) ||
        (ps.u8_buffer[0] == 0xe0 && c8 < 0xa0) ||
        (ps.u8_buffer[0] == 0xed && c8 > 0x9f) ||
        (ps.u8_buffer[0] == 0xf0 && c8 < 0x90) ||
        (ps.u8_buffer[0] == 0xf4 && c8 > 0xbf)
      {
        //errno::set_errno(errno::EILSEQ);
        return -1isize as size_t;
      }

      if ps.u8_buffer[0] >= 0xe0 {
        ps.u8_buffer[ps.u8_position] = c8;
        ps.u8_position += 1;
        return 0;
      }
    } else {
      if c8 < 0x80 || c8 > 0xbf {
        //errno::set_errno(errno::EILSEQ);
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
          return c32tomb(s, c32 as char32_t) as size_t;
        }
        decoded.len()
      },
      | Err(_) => {
        //errno::set_errno(errno::EILSEQ);
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

  if ps.u16_surrogate != 0 {
    let units = [ps.u16_surrogate, c16];
    let mut decoder = char::decode_utf16(units.iter().copied());

    match decoder.next() {
      | Some(Ok(c)) => {
        ps.reset();
        return c32tomb(s, c as char32_t) as size_t;
      },
      | _ => {
        //errno::set_errno(errno::EILSEQ);
        return -1isize as size_t;
      }
    }
  } else {
    let units = [c16];
    let mut decoder = char::decode_utf16(units.iter().copied());

    if let Some(next) = decoder.next() {
      match next {
        | Ok(c) => {
          ps.reset();
          return c32tomb(s, c as char32_t) as size_t;
        },
        | Err(e) => {
          if (0xd800..=0xdbff).contains(&e.unpaired_surrogate()) {
            ps.u16_surrogate = e.unpaired_surrogate();
            return 0;
          }
        },
      }
    }
  }

  //errno::set_errno(errno::EILSEQ);
  -1isize as size_t
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
  c32tomb(s, c32) as size_t
}

#[unsafe(no_mangle)]
pub extern "C" fn rs_mbrtoc8(
  pc8: *mut char8_t,
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

  let rc8 = pc8;
  let mut c8: char8_t = 0;
  let (pc8, buffer): (&mut char8_t, &[u8]) = if s.is_null() {
    unsafe { (&mut *pc8, [0u8; 1].as_slice()) }
  } else if pc8.is_null() {
    unsafe { (&mut c8, core::slice::from_raw_parts(s as *const u8, n)) }
  } else {
    unsafe { (&mut *pc8, core::slice::from_raw_parts(s as *const u8, n)) }
  };

  if ps.u8_position != 0 {
    if !rc8.is_null() {
      let total = ps.u8_buffer.iter().position(|&b| b == 0).unwrap_or(4);
      let index = total - ps.u8_position;

      *pc8 = ps.u8_buffer[index];
    }
    ps.u8_position -= 1;
    return -3isize as usize;
  }

  let mut c32: char32_t = 0;
  let l: ssize_t = mbtoc32(&mut c32, buffer, ps);
  if l >= 0 {
    match l {
      | 0 => {
        if !rc8.is_null() {
          *pc8 = 0;
        }
        return 0;
      },
      | -1 | -2 => return l as size_t,
      | _ => {}
    }

    let decoded = match char::from_u32(c32) {
      | Some(d) => d,
      | None => {
        //errno::set_errno(errno::EILSEQ);
        return -1isize as usize;
      }
    };

    let mut buffer = [0u8; 4];
    let result = decoded.encode_utf8(&mut buffer).as_bytes();

    ps.u8_buffer[..result.len()].copy_from_slice(result);
    ps.u8_position = result.len() - 1;

    if !rc8.is_null() {
      *pc8 = ps.u8_buffer[0];
    }

    if *pc8 == b'\0' {
      return 0;
    }
  }

  l as size_t
}

#[unsafe(no_mangle)]
pub extern "C" fn rs_mbrtoc16(
  pc16: *mut char16_t,
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

  let rc16 = pc16;
  let mut c16: char16_t = 0;
  let (pc16, buffer): (&mut char16_t, &[u8]) = if s.is_null() {
    unsafe { (&mut *pc16, [0u8; 1].as_slice()) }
  } else if pc16.is_null() {
    unsafe { (&mut c16, core::slice::from_raw_parts(s as *const u8, n)) }
  } else {
    unsafe { (&mut *pc16, core::slice::from_raw_parts(s as *const u8, n)) }
  };

  if ps.u16_surrogate != 0 {
    if !rc16.is_null() {
      *pc16 = ps.u16_surrogate;
    }
    ps.u16_surrogate = 0;
    return -3isize as size_t;
  }

  let mut c32: char32_t = 0;
  let l: ssize_t = mbtoc32(&mut c32, buffer, ps);
  if l >= 0 {
    match l {
      | 0 => {
        if !rc16.is_null() {
          *pc16 = 0;
        }
        return 0;
      },
      | -1 | -2 => return l as size_t,
      | _ => {}
    }

    let decoded = match char::from_u32(c32) {
      | Some(d) => d,
      | None => {
        //errno::set_errno(errno::EILSEQ);
        return -1isize as usize;
      }
    };

    let mut buffer = [0u16; 16];
    let result = decoded.encode_utf16(&mut buffer);

    ps.u16_buffer[..result.len()].copy_from_slice(result);

    if result.len() == 2 {
      let leading = ps.u16_buffer[0];
      let trailing = ps.u16_buffer[1];

      ps.u16_surrogate = trailing;
      if !rc16.is_null() {
        *pc16 = leading;
      }
    } else {
      if !rc16.is_null() {
        *pc16 = ps.u16_buffer[0];
      }
    }

    if *pc16 == '\0' as char16_t {
      return 0;
    }
  }

  l as size_t
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

  let mut c32: char32_t = 0;
  let (pc32, buffer): (&mut char32_t, &[u8]) = if s.is_null() {
    unsafe { (&mut *pc32, [0u8; 1].as_slice()) }
  } else if pc32.is_null() {
    unsafe { (&mut c32, core::slice::from_raw_parts(s as *const u8, n)) }
  } else {
    unsafe { (&mut *pc32, core::slice::from_raw_parts(s as *const u8, n)) }
  };

  let l: ssize_t = mbtoc32(pc32, buffer, ps);
  if l >= 0 && *pc32 == '\0' as char32_t {
    return 0;
  }
  l as size_t
}

#[unsafe(no_mangle)]
pub extern "C" fn rs_mbsinit(ps: *const mbstate_t) -> c_int {
  if ps.is_null() {
    c_int::from(true)
  } else {
    let ps = unsafe { *ps as MBState };
    c_int::from(ps.is_initial())
  }
}

// BOILERPLATE
fn c32tomb(
  s: *mut c_char,
  c32: char32_t
) -> ssize_t {
  let mut s = s;
  unsafe {
    if c32 <= 0x7f {
      *s = c32 as c_char;
      return 1;
    } else if c32 <= 0x7ff {
      *s = 0xc0u8 as c_char | (c32.wrapping_shr(6)) as c_char;
      s = s.wrapping_offset(1);
      *s = 0x80u8 as c_char | (c32 & 0x3f) as c_char;
      return 2;
    } else if c32 <= 0xffff {
      if c32 >= 0xd800 && c32 <= 0xdfff {
        //errno::set_errno(errno::EILSEQ);
        return -1;
      }
      *s = 0xe0u8 as c_char | (c32.wrapping_shr(12)) as c_char;
      s = s.wrapping_offset(1);
      *s = 0x80u8 as c_char | ((c32.wrapping_shr(6)) & 0x3f) as c_char;
      s = s.wrapping_offset(1);
      *s = 0x80u8 as c_char | (c32 & 0x3f) as c_char;
      return 3;
    } else if c32 <= 0x10ffff {
      *s = 0xf0u8 as c_char | (c32.wrapping_shr(18)) as c_char;
      s = s.wrapping_offset(1);
      *s = 0x80u8 as c_char | ((c32.wrapping_shr(12)) & 0x3f) as c_char;
      s = s.wrapping_offset(1);
      *s = 0x80u8 as c_char | ((c32.wrapping_shr(6)) & 0x3f) as c_char;
      s = s.wrapping_offset(1);
      *s = 0x80u8 as c_char | (c32 & 0x3f) as c_char;
      return 4;
    } else {
      //errno::set_errno(errno::EILSEQ);
      return -1;
    }
  }
}

fn mbtoc32(
  pc32: &mut char32_t,
  s: &[u8],
  ps: &mut MBState
) -> ssize_t {
  let mut n = s.len();
  let mut offset = 0;

  if n < 1 {
    return -2;
  }

  let mut bytesleft = ps.bytesleft;
  let mut partial = ps.partial;
  let mut lowerbound = ps.lowerbound;

  if bytesleft == 0 {
    if (s[offset] & 0x80) == 0 {
      *pc32 = s[offset] as char32_t;
      ps.reset();
      return 1;
    } else if (s[offset] & 0xe0) == 0xc0 {
      bytesleft = 1;
      partial = s[offset] as char32_t & 0x1f;
      lowerbound = 0x80;
      offset += 1;
    } else if (s[offset] & 0xf0) == 0xe0 {
      bytesleft = 2;
      partial = s[offset] as char32_t & 0xf;
      lowerbound = 0x800;
      offset += 1;
    } else if (s[offset] & 0xf8) == 0xf0 {
      bytesleft = 3;
      partial = s[offset] as char32_t & 0x7;
      lowerbound = 0x10000;
      offset += 1;
    } else {
      //errno::set_errno(errno::EILSEQ);
      return -1;
    }

    n -= 1;
  }

  while n > 0 {
    if (s[offset] & 0xc0) != 0x80 {
      //errno::set_errno(errno::EILSEQ);
      return -1;
    }

    partial <<= 6;
    partial |= s[offset] as char32_t & 0x3f;
    offset += 1;
    bytesleft -= 1;

    if bytesleft == 0 {
      if partial < lowerbound ||
        (partial >= 0xd800 && partial <= 0xdfff) ||
        partial > 0x10ffff
      {
        //errno::set_errno(errno::EILSEQ);
        return -1;
      }

      *pc32 = partial;
      ps.reset();
      return offset as ssize_t;
    }

    n -= 1;
  }

  ps.bytesleft = bytesleft;
  ps.lowerbound = lowerbound;
  ps.partial = partial;

  -2
}
