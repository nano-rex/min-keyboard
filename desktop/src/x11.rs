//! A small X11 client speaking the core protocol over the local Unix socket: the requests the
//! XIM server and its candidate window need, with MIT-MAGIC-COOKIE-1 authentication.

use std::collections::{HashMap, VecDeque};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

pub type Event = [u8; 32];

pub const ATOM_ATOM: u32 = 4;
pub const ATOM_STRING: u32 = 31;

pub const EXPOSE: u8 = 12;
pub const BUTTON_PRESS: u8 = 4;
pub const DESTROY_NOTIFY: u8 = 17;
pub const SELECTION_CLEAR: u8 = 29;
pub const SELECTION_REQUEST: u8 = 30;
pub const SELECTION_NOTIFY: u8 = 31;
pub const CLIENT_MESSAGE: u8 = 33;
pub const MAPPING_NOTIFY: u8 = 34;

pub const PROP_REPLACE: u8 = 0;
pub const PROP_PREPEND: u8 = 1;
pub const PROP_APPEND: u8 = 2;

pub fn u16_at(data: &[u8], at: usize) -> u16 {
    data.get(at..at + 2).map_or(0, |b| u16::from_le_bytes([b[0], b[1]]))
}

pub fn i16_at(data: &[u8], at: usize) -> i16 {
    u16_at(data, at) as i16
}

pub fn u32_at(data: &[u8], at: usize) -> u32 {
    data.get(at..at + 4).map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn pad4(n: usize) -> usize {
    (4 - n % 4) % 4
}

/// Builds one request; the length field is filled in by [`X11::send`].
pub struct Request(Vec<u8>);

impl Request {
    pub fn new(opcode: u8, data: u8) -> Request {
        Request(vec![opcode, data, 0, 0])
    }

    pub fn u8(mut self, v: u8) -> Self {
        self.0.push(v);
        self
    }

    pub fn u16(mut self, v: u16) -> Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }

    pub fn i16(self, v: i16) -> Self {
        self.u16(v as u16)
    }

    pub fn u32(mut self, v: u32) -> Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }

    pub fn bytes(mut self, data: &[u8]) -> Self {
        self.0.extend_from_slice(data);
        let pad = pad4(self.0.len());
        self.0.extend(std::iter::repeat_n(0, pad));
        self
    }
}

#[derive(Clone, Debug, Default)]
pub struct Setup {
    pub resource_base: u32,
    pub resource_mask: u32,
    pub max_request_bytes: usize,
    pub min_keycode: u8,
    pub max_keycode: u8,
    pub root: u32,
    pub root_visual: u32,
    pub root_depth: u8,
    pub width: u16,
    pub height: u16,
    /// Whether images can be sent as 32-bit little-endian pixels at the root depth.
    pub images_32bit: bool,
}

pub struct X11 {
    reader: BufReader<UnixStream>,
    writer: UnixStream,
    sequence: u16,
    next_resource: u32,
    events: VecDeque<Event>,
    atoms: HashMap<String, u32>,
    pub setup: Setup,
}

struct Display {
    number: String,
}

fn parse_display(display: &str) -> io::Result<Display> {
    let (host, rest) = display.rsplit_once(':').ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, format!("bad DISPLAY {display:?}")))?;
    if !(host.is_empty() || host == "unix" || host.starts_with('/')) {
        return Err(io::Error::new(io::ErrorKind::Unsupported, format!("DISPLAY {display:?} is remote; only local displays are supported")));
    }
    let number = rest.split('.').next().unwrap_or("0").to_string();
    if number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, format!("bad DISPLAY {display:?}")));
    }
    Ok(Display { number })
}

fn connect_socket(number: &str) -> io::Result<UnixStream> {
    let path = format!("/tmp/.X11-unix/X{number}");
    #[cfg(target_os = "linux")]
    {
        use std::os::linux::net::SocketAddrExt;
        if let Ok(addr) = std::os::unix::net::SocketAddr::from_abstract_name(path.as_bytes()) {
            if let Ok(stream) = UnixStream::connect_addr(&addr) {
                return Ok(stream);
            }
        }
    }
    UnixStream::connect(path)
}

/// Finds the MIT-MAGIC-COOKIE-1 for the display in $XAUTHORITY or ~/.Xauthority.
fn cookie(number: &str) -> Option<Vec<u8>> {
    let path = std::env::var_os("XAUTHORITY").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".Xauthority")))?;
    let data = std::fs::read(path).ok()?;
    let hostname = std::fs::read_to_string("/proc/sys/kernel/hostname").unwrap_or_default().trim().to_string();
    let mut at = 0;
    let mut fallback = None;
    while at < data.len() {
        let family = read_be16(&data, &mut at)?;
        let address = read_field(&data, &mut at)?;
        let display = read_field(&data, &mut at)?;
        let name = read_field(&data, &mut at)?;
        let secret = read_field(&data, &mut at)?;
        if name != b"MIT-MAGIC-COOKIE-1" || !(display.is_empty() || display == number.as_bytes()) {
            continue;
        }
        // FamilyLocal (256) for this host is exact; FamilyWild (65535) matches any host.
        if family == 256 && address == hostname.as_bytes() {
            return Some(secret);
        }
        if family == 65535 || (family == 256 && fallback.is_none()) {
            fallback = Some(secret);
        }
    }
    fallback
}

fn read_be16(data: &[u8], at: &mut usize) -> Option<u16> {
    let value = u16::from_be_bytes(data.get(*at..*at + 2)?.try_into().ok()?);
    *at += 2;
    Some(value)
}

fn read_field(data: &[u8], at: &mut usize) -> Option<Vec<u8>> {
    let len = read_be16(data, at)? as usize;
    let value = data.get(*at..*at + len)?.to_vec();
    *at += len;
    Some(value)
}

impl X11 {
    pub fn connect() -> io::Result<X11> {
        let display = std::env::var("DISPLAY").map_err(|_| io::Error::new(io::ErrorKind::NotFound, "DISPLAY is not set"))?;
        let display = parse_display(&display)?;
        let stream = connect_socket(&display.number)?;
        let mut writer = stream.try_clone()?;
        let mut reader = BufReader::new(stream);

        let (name, secret): (&[u8], Vec<u8>) = match cookie(&display.number) {
            Some(secret) => (b"MIT-MAGIC-COOKIE-1", secret),
            None => (b"", Vec::new()),
        };
        let mut hello = vec![b'l', 0];
        hello.extend_from_slice(&11u16.to_le_bytes());
        hello.extend_from_slice(&0u16.to_le_bytes());
        hello.extend_from_slice(&(name.len() as u16).to_le_bytes());
        hello.extend_from_slice(&(secret.len() as u16).to_le_bytes());
        hello.extend_from_slice(&[0, 0]);
        hello.extend_from_slice(name);
        hello.extend(std::iter::repeat_n(0, pad4(name.len())));
        hello.extend_from_slice(&secret);
        hello.extend(std::iter::repeat_n(0, pad4(secret.len())));
        writer.write_all(&hello)?;

        let mut head = [0u8; 8];
        reader.read_exact(&mut head)?;
        let mut body = vec![0u8; u16_at(&head, 6) as usize * 4];
        reader.read_exact(&mut body)?;
        if head[0] != 1 {
            let reason = String::from_utf8_lossy(body.get(..head[1] as usize).unwrap_or_default()).into_owned();
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, format!("X server refused the connection: {}", reason.trim())));
        }
        let setup = Self::parse_setup(&body).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "bad X setup reply"))?;
        Ok(X11 { reader, writer, sequence: 0, next_resource: 0, events: VecDeque::new(), atoms: HashMap::new(), setup })
    }

    fn parse_setup(d: &[u8]) -> Option<Setup> {
        let vendor_len = u16_at(d, 16) as usize;
        let formats = d[21] as usize;
        let mut setup = Setup {
            resource_base: u32_at(d, 4),
            resource_mask: u32_at(d, 8),
            max_request_bytes: u16_at(d, 18) as usize * 4,
            min_keycode: d[26],
            max_keycode: d[27],
            ..Setup::default()
        };
        let image_lsb = d[22] == 0;
        let formats_at = 32 + vendor_len + pad4(vendor_len);
        let screen = formats_at + 8 * formats;
        d.get(screen + 40)?;
        setup.root = u32_at(d, screen);
        setup.width = u16_at(d, screen + 20);
        setup.height = u16_at(d, screen + 22);
        setup.root_visual = u32_at(d, screen + 32);
        setup.root_depth = d[screen + 38];
        let bpp = (0..formats).map(|i| &d[formats_at + 8 * i..formats_at + 8 * i + 2]).find(|f| f[0] == setup.root_depth).map(|f| f[1]);
        setup.images_32bit = image_lsb && bpp == Some(32) && setup.root_depth >= 24;
        Some(setup)
    }

    pub fn new_id(&mut self) -> u32 {
        let mask = self.setup.resource_mask;
        let step = mask & mask.wrapping_neg();
        self.next_resource += step;
        self.setup.resource_base | (self.next_resource & mask)
    }

    pub fn send(&mut self, request: Request) -> io::Result<u16> {
        let mut bytes = request.0;
        let pad = pad4(bytes.len());
        bytes.extend(std::iter::repeat_n(0, pad));
        let units = (bytes.len() / 4) as u16;
        bytes[2..4].copy_from_slice(&units.to_le_bytes());
        self.writer.write_all(&bytes)?;
        self.sequence = self.sequence.wrapping_add(1);
        Ok(self.sequence)
    }

    /// Reads one packet: a reply or error (`Err` with the sequence) or an event.
    fn read_packet(&mut self) -> io::Result<(u8, Vec<u8>)> {
        let mut packet = vec![0u8; 32];
        self.reader.read_exact(&mut packet)?;
        let extra = match packet[0] {
            1 => u32_at(&packet, 4) as usize * 4,
            35 => u32_at(&packet, 4) as usize * 4,
            _ => 0,
        };
        if extra > 0 {
            if extra > 64 << 20 {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "X reply too large"));
            }
            let mut rest = vec![0u8; extra];
            self.reader.read_exact(&mut rest)?;
            packet.extend_from_slice(&rest);
        }
        Ok((packet[0], packet))
    }

    /// Waits for the reply to request `sequence`, keeping events that arrive meanwhile.
    pub fn reply(&mut self, sequence: u16) -> io::Result<Vec<u8>> {
        loop {
            let (kind, packet) = self.read_packet()?;
            match kind {
                0 if u16_at(&packet, 2) == sequence => {
                    return Err(io::Error::other(format!("X error {} for request {} (resource {:#x})", packet[1], packet[10], u32_at(&packet, 4))));
                }
                0 => eprintln!("min-keyboard: X error {} for request {} (resource {:#x})", packet[1], packet[10], u32_at(&packet, 4)),
                1 if u16_at(&packet, 2) == sequence => return Ok(packet),
                1 | 35 => {}
                _ => self.events.push_back(packet[..32].try_into().expect("32 bytes")),
            }
        }
    }

    /// The next event, waiting at most `timeout` (forever when `None`); `Ok(None)` on timeout.
    pub fn next_event(&mut self, timeout: Option<Duration>) -> io::Result<Option<Event>> {
        loop {
            if let Some(event) = self.events.pop_front() {
                return Ok(Some(event));
            }
            if self.reader.buffer().is_empty() {
                if let Some(timeout) = timeout {
                    self.reader.get_ref().set_read_timeout(Some(timeout.max(Duration::from_millis(1))))?;
                    let filled = self.reader.fill_buf().map(|b| b.len());
                    self.reader.get_ref().set_read_timeout(None)?;
                    match filled {
                        Ok(0) => return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "X server closed the connection")),
                        Ok(_) => {}
                        Err(e) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) => return Ok(None),
                        Err(e) => return Err(e),
                    }
                }
            }
            let (kind, packet) = self.read_packet()?;
            match kind {
                0 => eprintln!("min-keyboard: X error {} for request {} (resource {:#x})", packet[1], packet[10], u32_at(&packet, 4)),
                1 | 35 => {}
                _ => return Ok(Some(packet[..32].try_into().expect("32 bytes"))),
            }
        }
    }

    pub fn atom(&mut self, name: &str) -> io::Result<u32> {
        if let Some(&atom) = self.atoms.get(name) {
            return Ok(atom);
        }
        let seq = self.send(Request::new(16, 0).u16(name.len() as u16).u16(0).bytes(name.as_bytes()))?;
        let atom = u32_at(&self.reply(seq)?, 8);
        self.atoms.insert(name.to_string(), atom);
        Ok(atom)
    }

    /// Creates an InputOutput window; `values` are (mask bit, value) pairs in bit order.
    #[allow(clippy::too_many_arguments)]
    pub fn create_window(&mut self, parent: u32, x: i16, y: i16, width: u16, height: u16, values: &[(u32, u32)]) -> io::Result<u32> {
        let id = self.new_id();
        let mask = values.iter().fold(0, |m, (bit, _)| m | bit);
        let mut request = Request::new(1, 0).u32(id).u32(parent).i16(x).i16(y).u16(width.max(1)).u16(height.max(1)).u16(0).u16(1).u32(0).u32(mask);
        for &(_, value) in values {
            request = request.u32(value);
        }
        self.send(request)?;
        Ok(id)
    }

    pub fn change_attributes(&mut self, window: u32, values: &[(u32, u32)]) -> io::Result<()> {
        let mask = values.iter().fold(0, |m, (bit, _)| m | bit);
        let mut request = Request::new(2, 0).u32(window).u32(mask);
        for &(_, value) in values {
            request = request.u32(value);
        }
        self.send(request).map(drop)
    }

    pub fn destroy_window(&mut self, window: u32) -> io::Result<()> {
        self.send(Request::new(4, 0).u32(window)).map(drop)
    }

    pub fn map_window(&mut self, window: u32) -> io::Result<()> {
        self.send(Request::new(8, 0).u32(window)).map(drop)
    }

    pub fn unmap_window(&mut self, window: u32) -> io::Result<()> {
        self.send(Request::new(10, 0).u32(window)).map(drop)
    }

    /// Moves, resizes and raises a window.
    pub fn configure(&mut self, window: u32, x: i32, y: i32, width: u32, height: u32) -> io::Result<()> {
        let request = Request::new(12, 0).u32(window).u16(0x1 | 0x2 | 0x4 | 0x8 | 0x40).u16(0).u32(x as u32).u32(y as u32).u32(width.max(1)).u32(height.max(1)).u32(0);
        self.send(request).map(drop)
    }

    /// The window's size.
    pub fn geometry(&mut self, window: u32) -> io::Result<(u16, u16)> {
        let seq = self.send(Request::new(14, 0).u32(window))?;
        let reply = self.reply(seq)?;
        Ok((u16_at(&reply, 16), u16_at(&reply, 18)))
    }

    /// Converts a point in `window` to root coordinates.
    pub fn to_root(&mut self, window: u32, x: i16, y: i16) -> io::Result<(i32, i32)> {
        let seq = self.send(Request::new(40, 0).u32(window).u32(self.setup.root).i16(x).i16(y))?;
        let reply = self.reply(seq)?;
        Ok((i32::from(i16_at(&reply, 12)), i32::from(i16_at(&reply, 14))))
    }

    pub fn change_property(&mut self, window: u32, property: u32, kind: u32, format: u8, mode: u8, data: &[u8]) -> io::Result<()> {
        let units = data.len() / (format as usize / 8).max(1);
        let request = Request::new(18, mode).u32(window).u32(property).u32(kind).u8(format).u8(0).u16(0).u32(units as u32).bytes(data);
        self.send(request).map(drop)
    }

    /// Reads up to `max_bytes` of a property, returning its type and data.
    pub fn get_property(&mut self, window: u32, property: u32, delete: bool, max_bytes: usize) -> io::Result<(u32, Vec<u8>)> {
        let seq = self.send(Request::new(20, u8::from(delete)).u32(window).u32(property).u32(0).u32(0).u32(max_bytes.div_ceil(4) as u32))?;
        let reply = self.reply(seq)?;
        let format = reply[1] as usize;
        let len = u32_at(&reply, 16) as usize * (format / 8);
        Ok((u32_at(&reply, 8), reply.get(32..32 + len).unwrap_or_default().to_vec()))
    }

    pub fn set_selection_owner(&mut self, owner: u32, selection: u32) -> io::Result<()> {
        self.send(Request::new(22, 0).u32(owner).u32(selection).u32(0)).map(drop)
    }

    pub fn selection_owner(&mut self, selection: u32) -> io::Result<u32> {
        let seq = self.send(Request::new(23, 0).u32(selection))?;
        Ok(u32_at(&self.reply(seq)?, 8))
    }

    pub fn send_event(&mut self, destination: u32, mask: u32, event: &Event) -> io::Result<()> {
        self.send(Request::new(25, 0).u32(destination).u32(mask).bytes(event)).map(drop)
    }

    pub fn create_gc(&mut self, drawable: u32) -> io::Result<u32> {
        let id = self.new_id();
        self.send(Request::new(55, 0).u32(id).u32(drawable).u32(0))?;
        Ok(id)
    }

    /// Draws 32-bit pixels, split into requests that fit the server's maximum request size.
    pub fn put_image(&mut self, window: u32, gc: u32, width: usize, height: usize, pixels: &[u32]) -> io::Result<()> {
        if width == 0 || !self.setup.images_32bit {
            return Ok(());
        }
        let rows_per_request = ((self.setup.max_request_bytes.saturating_sub(64)) / (width * 4)).clamp(1, height.max(1));
        let depth = self.setup.root_depth;
        let mut row = 0;
        while row < height {
            let rows = rows_per_request.min(height - row);
            let mut data = Vec::with_capacity(rows * width * 4);
            for pixel in &pixels[row * width..(row + rows) * width] {
                data.extend_from_slice(&pixel.to_le_bytes());
            }
            let request = Request::new(72, 2).u32(window).u32(gc).u16(width as u16).u16(rows as u16).i16(0).i16(row as i16).u8(0).u8(depth).u16(0).bytes(&data);
            self.send(request)?;
            row += rows;
        }
        Ok(())
    }

    /// The keysyms of every keycode: (keysyms per keycode, table from the minimum keycode).
    pub fn keyboard_mapping(&mut self) -> io::Result<(usize, Vec<u32>)> {
        let (min, max) = (self.setup.min_keycode, self.setup.max_keycode);
        let seq = self.send(Request::new(101, 0).u8(min).u8(max - min + 1).u16(0))?;
        let reply = self.reply(seq)?;
        let per = reply[1] as usize;
        let count = u32_at(&reply, 4) as usize;
        Ok((per, (0..count).map(|i| u32_at(&reply, 32 + 4 * i)).collect()))
    }

    pub fn flush(&mut self) -> io::Result<()> {
        self.writer.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn displays() {
        assert_eq!(parse_display(":0").unwrap().number, "0");
        assert_eq!(parse_display(":1.0").unwrap().number, "1");
        assert_eq!(parse_display("unix:2").unwrap().number, "2");
        assert!(parse_display("remote:0").is_err());
        assert!(parse_display("junk").is_err());
    }

    #[test]
    fn requests_are_padded() {
        let request = Request::new(16, 0).u16(3).u16(0).bytes(b"abc");
        assert_eq!(request.0.len(), 12);
    }
}
