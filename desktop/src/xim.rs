//! An XIM server: X applications that use the X Input Method protocol (GTK with
//! GTK_IM_MODULE=xim, Firefox, Chromium, LibreOffice, xterm, …) connect to it when
//! XMODIFIERS=@im=minkeyboard. It uses the static event flow (applications forward every key
//! press) and draws the composition and candidates in its own window at the text cursor.

use std::collections::HashMap;
use std::io;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::core::Core;
use crate::font::Fonts;
use crate::keysym;
use crate::popup::{self, Popup, Theme};
use crate::session::Session;
use crate::x11::{self, i16_at, u16_at, u32_at, Event, X11};

pub const SERVER_NAME: &str = "minkeyboard";

// XIM opcodes.
const CONNECT: u8 = 1;
const CONNECT_REPLY: u8 = 2;
const DISCONNECT: u8 = 3;
const DISCONNECT_REPLY: u8 = 4;
const ERROR: u8 = 20;
const OPEN: u8 = 30;
const OPEN_REPLY: u8 = 31;
const CLOSE: u8 = 32;
const CLOSE_REPLY: u8 = 33;
const TRIGGER_NOTIFY: u8 = 35;
const TRIGGER_NOTIFY_REPLY: u8 = 36;
const SET_EVENT_MASK: u8 = 37;
const ENCODING_NEGOTIATION: u8 = 38;
const ENCODING_NEGOTIATION_REPLY: u8 = 39;
const QUERY_EXTENSION: u8 = 40;
const QUERY_EXTENSION_REPLY: u8 = 41;
const SET_IM_VALUES: u8 = 42;
const SET_IM_VALUES_REPLY: u8 = 43;
const GET_IM_VALUES: u8 = 44;
const GET_IM_VALUES_REPLY: u8 = 45;
const CREATE_IC: u8 = 50;
const CREATE_IC_REPLY: u8 = 51;
const DESTROY_IC: u8 = 52;
const DESTROY_IC_REPLY: u8 = 53;
const SET_IC_VALUES: u8 = 54;
const SET_IC_VALUES_REPLY: u8 = 55;
const GET_IC_VALUES: u8 = 56;
const GET_IC_VALUES_REPLY: u8 = 57;
const SET_IC_FOCUS: u8 = 58;
const UNSET_IC_FOCUS: u8 = 59;
const FORWARD_EVENT: u8 = 60;
const SYNC: u8 = 61;
const SYNC_REPLY: u8 = 62;
const COMMIT: u8 = 63;
const RESET_IC: u8 = 64;
const RESET_IC_REPLY: u8 = 65;

const KEY_PRESS: u8 = 2;
const KEY_PRESS_MASK: u32 = 1;
const XIM_LOOKUP_CHARS: u16 = 2;
const XIM_SYNCHRONOUS: u16 = 1;

// Input styles offered: the composition is drawn by the server near the cursor (over the spot)
// or wherever it can be (root window style). On-the-spot callbacks are not offered, so
// applications send the cursor position.
const STYLES: [u32; 4] = [0x0404, 0x0804, 0x0408, 0x0808];

// IC attribute ids, the index in IC_ATTRIBUTES.
const IC_INPUT_STYLE: u16 = 0;
const IC_CLIENT_WINDOW: u16 = 1;
const IC_FOCUS_WINDOW: u16 = 2;
const IC_FILTER_EVENTS: u16 = 3;
const IC_SPOT_LOCATION: u16 = 14;
const IC_SEPARATOR: u16 = 17;

const T_SEPARATOR: u16 = 0;
const T_CARD32: u16 = 3;
const T_WINDOW: u16 = 5;
const T_STYLES: u16 = 10;
const T_RECTANGLE: u16 = 11;
const T_POINT: u16 = 12;
const T_FONTSET: u16 = 13;
const T_NEST: u16 = 0x7fff;

const IC_ATTRIBUTES: [(&str, u16); 20] = [
    ("inputStyle", T_CARD32),
    ("clientWindow", T_WINDOW),
    ("focusWindow", T_WINDOW),
    ("filterEvents", T_CARD32),
    ("preeditAttributes", T_NEST),
    ("statusAttributes", T_NEST),
    ("fontSet", T_FONTSET),
    ("area", T_RECTANGLE),
    ("areaNeeded", T_RECTANGLE),
    ("colorMap", T_CARD32),
    ("stdColorMap", T_CARD32),
    ("foreground", T_CARD32),
    ("background", T_CARD32),
    ("backgroundPixmap", T_CARD32),
    ("spotLocation", T_POINT),
    ("lineSpace", T_CARD32),
    ("cursor", T_CARD32),
    ("separatorofNestedList", T_SEPARATOR),
    ("resetState", T_CARD32),
    ("preeditState", T_CARD32),
];

/// Logs XIM traffic to stderr when MIN_KEYBOARD_DEBUG is set.
fn debug() -> bool {
    static DEBUG: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *DEBUG.get_or_init(|| std::env::var_os("MIN_KEYBOARD_DEBUG").is_some())
}

fn pad4(n: usize) -> usize {
    (4 - n % 4) % 4
}

/// Little-endian XIM message body builder.
#[derive(Default)]
struct Body(Vec<u8>);

impl Body {
    fn u16(mut self, v: u16) -> Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    fn i16(self, v: i16) -> Self {
        self.u16(v as u16)
    }
    fn u32(mut self, v: u32) -> Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    fn bytes(mut self, data: &[u8]) -> Self {
        self.0.extend_from_slice(data);
        self
    }
    fn pad(mut self) -> Self {
        let pad = pad4(self.0.len());
        self.0.extend(std::iter::repeat_n(0, pad));
        self
    }
}

fn message(major: u8, body: Body) -> Vec<u8> {
    let body = body.pad().0;
    let mut out = vec![major, 0];
    out.extend_from_slice(&((body.len() / 4) as u16).to_le_bytes());
    out.extend_from_slice(&body);
    out
}

/// An attribute list (XIMATTR / XICATTR): id, type, name.
fn attribute_list(attributes: &[(&str, u16)]) -> Vec<u8> {
    let mut out = Vec::new();
    for (id, (name, kind)) in attributes.iter().enumerate() {
        out.extend_from_slice(&(id as u16).to_le_bytes());
        out.extend_from_slice(&kind.to_le_bytes());
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(name.as_bytes());
        out.extend(std::iter::repeat_n(0, pad4(2 + name.len())));
    }
    out
}

/// One attribute value (XIMATTRIBUTE / XICATTRIBUTE): id, length, value, padding.
fn attribute_value(id: u16, value: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&id.to_le_bytes());
    out.extend_from_slice(&(value.len() as u16).to_le_bytes());
    out.extend_from_slice(value);
    out.extend(std::iter::repeat_n(0, pad4(value.len())));
    out
}

/// Parses an XICATTRIBUTE list into (id, value) pairs, flattening nested lists.
fn parse_attributes(data: &[u8], out: &mut Vec<(u16, Vec<u8>)>, depth: u32) {
    let mut at = 0;
    while at + 4 <= data.len() {
        let id = u16_at(data, at);
        let len = u16_at(data, at + 2) as usize;
        let Some(value) = data.get(at + 4..at + 4 + len) else { return };
        at += 4 + len + pad4(len);
        match IC_ATTRIBUTES.get(id as usize) {
            Some((_, T_NEST)) if depth < 4 => parse_attributes(value, out, depth + 1),
            _ => out.push((id, value.to_vec())),
        }
    }
}

/// Encodes committed text. Applications that negotiated COMPOUND_TEXT get ASCII as it is and
/// anything else as a UTF-8 extended segment (ESC % G), which libX11 decodes.
pub fn encode_text(text: &str, utf8: bool) -> Vec<u8> {
    if utf8 || text.is_ascii() {
        text.as_bytes().to_vec()
    } else {
        let mut out = b"\x1b%G".to_vec();
        out.extend_from_slice(text.as_bytes());
        out
    }
}

fn locales() -> String {
    let mut names: Vec<String> = ["C", "POSIX"].iter().map(|s| s.to_string()).collect();
    const LANGUAGES: &str = "af am an ar as ast az be bg bn bo br bs ca cs cy da de dz el en eo es et eu fa fi fil fo fr fy ga gd gl gu gv he hi hr hsb ht hu hy ia id is it ja ka kk km kn ko ku kw ky lb lg li lo lt lv mai mg mi mk ml mn mr ms mt my nb ne nl nn nr oc om or pa pl ps pt ro ru rw sa sc sd se si sk sl so sq sr ss st sv sw ta te tg th ti tk tl tn tr ts tt ug uk ur uz ve vi wa xh yi yo zh zu";
    names.extend(LANGUAGES.split(' ').map(str::to_string));
    for extra in ["en_US", "en_GB", "en_AU", "en_CA", "en_IN", "en_SG", "en_MY", "ms_MY", "zh_CN", "zh_TW", "zh_HK", "zh_SG", "ja_JP", "ko_KR"] {
        names.push(extra.to_string());
    }
    for variable in ["LC_ALL", "LC_CTYPE", "LANG"] {
        if let Ok(value) = std::env::var(variable) {
            if let Some(base) = value.split(['.', '@']).next().filter(|b| !b.is_empty()) {
                names.push(base.to_string());
            }
            break;
        }
    }
    names.dedup();
    format!("@locale={}", names.join(","))
}

#[derive(Default)]
struct Ic {
    style: u32,
    client_window: u32,
    focus_window: u32,
    spot: Option<(i16, i16)>,
}

impl Ic {
    fn apply(&mut self, attributes: &[(u16, Vec<u8>)]) {
        for (id, value) in attributes {
            match *id {
                IC_INPUT_STYLE => self.style = u32_at(value, 0),
                IC_CLIENT_WINDOW => self.client_window = u32_at(value, 0),
                IC_FOCUS_WINDOW => self.focus_window = u32_at(value, 0),
                IC_SPOT_LOCATION if value.len() >= 4 => self.spot = Some((i16_at(value, 0), i16_at(value, 2))),
                _ => {}
            }
        }
    }

    fn window(&self) -> u32 {
        if self.focus_window != 0 {
            self.focus_window
        } else {
            self.client_window
        }
    }

    /// The value of an attribute for XIM_GET_IC_VALUES.
    fn value(&self, id: u16) -> Vec<u8> {
        match id {
            IC_INPUT_STYLE => self.style.to_le_bytes().to_vec(),
            IC_CLIENT_WINDOW => self.client_window.to_le_bytes().to_vec(),
            IC_FOCUS_WINDOW => self.window().to_le_bytes().to_vec(),
            IC_FILTER_EVENTS => KEY_PRESS_MASK.to_le_bytes().to_vec(),
            IC_SPOT_LOCATION => {
                let (x, y) = self.spot.unwrap_or_default();
                [x.to_le_bytes(), y.to_le_bytes()].concat()
            }
            _ => match IC_ATTRIBUTES.get(id as usize).map(|a| a.1) {
                Some(T_RECTANGLE) => vec![0; 8],
                Some(T_POINT | T_FONTSET) => vec![0; 4],
                _ => vec![0; 4],
            },
        }
    }
}

#[derive(Default)]
struct Im {
    ics: HashMap<u16, Ic>,
    next_ic: u16,
    utf8: bool,
}

struct Client {
    /// The application's communication window, where messages to it go.
    window: u32,
    ims: HashMap<u16, Im>,
    next_im: u16,
    /// A message arriving in several ClientMessage pieces (_XIM_MOREDATA).
    partial: Vec<u8>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Focus {
    client: u32,
    im: u16,
    ic: u16,
}

struct Atoms {
    server: u32,
    xim_servers: u32,
    locales: u32,
    transport: u32,
    xconnect: u32,
    protocol: u32,
    moredata: u32,
}

struct Window {
    id: u32,
    gc: u32,
    mapped: bool,
    geometry: (i32, i32, usize, usize),
    popup: Option<Popup>,
}

pub struct Server {
    x: X11,
    core: Arc<Mutex<Core>>,
    atoms: Atoms,
    window: u32,
    /// Our communication window for each client, and the client's window to ours.
    clients: HashMap<u32, Client>,
    by_client_window: HashMap<u32, u32>,
    focus: Option<Focus>,
    session: Session,
    keymap: (usize, Vec<u32>),
    popup: Window,
    fonts: Fonts,
    theme: Theme,
    notice: Option<(String, Instant)>,
    property_counter: u32,
}

fn lock(core: &Mutex<Core>) -> std::sync::MutexGuard<'_, Core> {
    core.lock().unwrap_or_else(|e| e.into_inner())
}

impl Server {
    pub fn start(core: Arc<Mutex<Core>>) -> io::Result<Server> {
        let mut x = X11::connect()?;
        let root = x.setup.root;
        let atoms = Atoms {
            server: x.atom(&format!("@server={SERVER_NAME}"))?,
            xim_servers: x.atom("XIM_SERVERS")?,
            locales: x.atom("LOCALES")?,
            transport: x.atom("TRANSPORT")?,
            xconnect: x.atom("_XIM_XCONNECT")?,
            protocol: x.atom("_XIM_PROTOCOL")?,
            moredata: x.atom("_XIM_MOREDATA")?,
        };
        let owner = x.selection_owner(atoms.server)?;
        if owner != 0 {
            return Err(io::Error::new(io::ErrorKind::AddrInUse, format!("another XIM server named {SERVER_NAME} is running")));
        }
        let window = x.create_window(root, -1, -1, 1, 1, &[])?;
        x.set_selection_owner(window, atoms.server)?;
        if x.selection_owner(atoms.server)? != window {
            return Err(io::Error::new(io::ErrorKind::AddrInUse, "could not own the XIM server selection"));
        }
        // Announce the server in XIM_SERVERS on the root window, without duplicating the entry.
        let (_, servers) = x.get_property(root, atoms.xim_servers, false, 4096)?;
        let listed = servers.chunks_exact(4).any(|a| u32_at(a, 0) == atoms.server);
        if listed {
            x.change_property(root, atoms.xim_servers, x11::ATOM_ATOM, 32, x11::PROP_PREPEND, &[])?;
        } else {
            x.change_property(root, atoms.xim_servers, x11::ATOM_ATOM, 32, x11::PROP_APPEND, &atoms.server.to_le_bytes())?;
        }

        let (dark, size, fonts_config) = {
            let core = lock(&core);
            (core.config.dark, core.config.font_size, core.config.fonts.clone())
        };
        let theme = Theme::new(dark);
        let fonts = Fonts::discover(&fonts_config, size);
        // Override-redirect so window managers leave it alone; exposure and clicks only.
        let popup_window = x.create_window(root, 0, 0, 1, 1, &[(0x2, 0xffff_ffff), (0x200, 1), (0x800, 0x8000 | 0x4)])?;
        let gc = x.create_gc(popup_window)?;
        if !x.setup.images_32bit {
            eprintln!("min-keyboard: the X display is not 24/32-bit TrueColor; the candidate window stays blank");
        }
        let keymap = x.keyboard_mapping()?;
        x.flush()?;
        Ok(Server {
            x,
            core,
            atoms,
            window,
            clients: HashMap::new(),
            by_client_window: HashMap::new(),
            focus: None,
            session: Session::default(),
            keymap,
            popup: Window { id: popup_window, gc, mapped: false, geometry: (0, 0, 0, 0), popup: None },
            fonts,
            theme,
            notice: None,
            property_counter: 0,
        })
    }

    pub fn run(mut self) -> io::Result<()> {
        loop {
            let timeout = self.notice.as_ref().map(|(_, until)| until.saturating_duration_since(Instant::now()));
            match self.x.next_event(timeout)? {
                None => {
                    self.notice = None;
                    self.refresh_popup()?;
                }
                Some(event) => {
                    if let Err(e) = self.handle_event(&event) {
                        if e.kind() == io::ErrorKind::ConnectionAborted {
                            return Err(e);
                        }
                        eprintln!("min-keyboard: {e}");
                    }
                }
            }
            self.x.flush()?;
        }
    }

    fn handle_event(&mut self, event: &Event) -> io::Result<()> {
        if debug() && event[0] & 0x7f != x11::CLIENT_MESSAGE {
            eprintln!("x event {} {:02x?}", event[0], &event[..16]);
        }
        match event[0] & 0x7f {
            x11::EXPOSE if u32_at(event, 4) == self.popup.id => self.draw(),
            x11::BUTTON_PRESS if u32_at(event, 12) == self.popup.id && event[1] == 1 => {
                let (px, py) = (i32::from(i16_at(event, 24)), i32::from(i16_at(event, 26)));
                let index = self.popup.popup.as_ref().and_then(|p| p.hit(px, py));
                if let (Some(index), Some(focus)) = (index, self.focus) {
                    let text = {
                        let core = lock(&self.core);
                        self.session.click(&core, index)
                    };
                    if let Some(text) = text {
                        self.commit(focus, &text)?;
                    }
                    self.refresh_popup()?;
                }
                Ok(())
            }
            x11::SELECTION_REQUEST => self.selection_request(event),
            x11::SELECTION_CLEAR if u32_at(event, 12) == self.atoms.server => {
                Err(io::Error::new(io::ErrorKind::ConnectionAborted, "another XIM server took over the selection"))
            }
            x11::CLIENT_MESSAGE => self.client_message(event),
            x11::DESTROY_NOTIFY => {
                let window = u32_at(event, 8);
                if let Some(ours) = self.by_client_window.remove(&window) {
                    self.drop_client(ours)?;
                }
                Ok(())
            }
            x11::MAPPING_NOTIFY if event[4] == 1 => {
                self.keymap = self.x.keyboard_mapping()?;
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn selection_request(&mut self, event: &Event) -> io::Result<()> {
        let (time, requestor, selection, target, property) = (u32_at(event, 4), u32_at(event, 12), u32_at(event, 16), u32_at(event, 20), u32_at(event, 24));
        let property = if property == 0 { target } else { property };
        let data = if selection != self.atoms.server {
            None
        } else if target == self.atoms.locales {
            Some(locales())
        } else if target == self.atoms.transport {
            Some("@transport=X/".to_string())
        } else {
            None
        };
        let reported = match data {
            Some(text) => {
                self.x.change_property(requestor, property, target, 8, x11::PROP_REPLACE, text.as_bytes())?;
                property
            }
            None => 0,
        };
        let mut notify = [0u8; 32];
        notify[0] = x11::SELECTION_NOTIFY;
        notify[4..8].copy_from_slice(&time.to_le_bytes());
        notify[8..12].copy_from_slice(&requestor.to_le_bytes());
        notify[12..16].copy_from_slice(&selection.to_le_bytes());
        notify[16..20].copy_from_slice(&target.to_le_bytes());
        notify[20..24].copy_from_slice(&reported.to_le_bytes());
        self.x.send_event(requestor, 0, &notify)
    }

    fn client_message(&mut self, event: &Event) -> io::Result<()> {
        let (format, window, kind) = (event[1], u32_at(event, 4), u32_at(event, 8));
        let data = &event[12..32];
        if window == self.window && kind == self.atoms.xconnect {
            return self.xconnect(u32_at(data, 0));
        }
        if !self.clients.contains_key(&window) {
            return Ok(());
        }
        if kind == self.atoms.moredata && format == 8 {
            if let Some(client) = self.clients.get_mut(&window) {
                client.partial.extend_from_slice(data);
            }
            return Ok(());
        }
        if kind != self.atoms.protocol {
            return Ok(());
        }
        let bytes = if format == 32 {
            let (len, property) = (u32_at(data, 0) as usize, u32_at(data, 4));
            let (_, mut bytes) = self.x.get_property(window, property, true, len)?;
            bytes.truncate(len);
            bytes
        } else {
            let client = self.clients.get_mut(&window).expect("checked above");
            let mut bytes = std::mem::take(&mut client.partial);
            bytes.extend_from_slice(data);
            bytes
        };
        let mut at = 0;
        while at + 4 <= bytes.len() {
            let len = 4 + 4 * u16_at(&bytes, at + 2) as usize;
            if bytes[at] == 0 || at + len > bytes.len() {
                break;
            }
            let message = bytes[at..at + len].to_vec();
            at += len;
            self.handle_xim(window, &message)?;
        }
        Ok(())
    }

    fn xconnect(&mut self, client_window: u32) -> io::Result<()> {
        if client_window == 0 {
            return Ok(());
        }
        let ours = self.x.create_window(self.x.setup.root, -1, -1, 1, 1, &[])?;
        // StructureNotify on the client's window tells us when the application goes away.
        self.x.change_attributes(client_window, &[(0x800, 0x2_0000)])?;
        self.clients.insert(ours, Client { window: client_window, ims: HashMap::new(), next_im: 0, partial: Vec::new() });
        self.by_client_window.insert(client_window, ours);
        let mut reply = [0u8; 32];
        reply[0] = x11::CLIENT_MESSAGE;
        reply[1] = 32;
        reply[4..8].copy_from_slice(&client_window.to_le_bytes());
        reply[8..12].copy_from_slice(&self.atoms.xconnect.to_le_bytes());
        // Our window, transport version 0.0 (ClientMessage, or a property for long messages),
        // and the 20-byte limit above which messages go through a property.
        reply[12..16].copy_from_slice(&ours.to_le_bytes());
        reply[24..28].copy_from_slice(&20u32.to_le_bytes());
        self.x.send_event(client_window, 0, &reply)
    }

    fn drop_client(&mut self, ours: u32) -> io::Result<()> {
        if self.focus.is_some_and(|f| f.client == ours) {
            self.focus = None;
            self.session.clear();
            self.hide()?;
        }
        if let Some(client) = self.clients.remove(&ours) {
            self.by_client_window.remove(&client.window);
        }
        self.x.destroy_window(ours)
    }

    /// Sends a message to a client: short ones in a ClientMessage, long ones through a property.
    fn send(&mut self, ours: u32, message: Vec<u8>) -> io::Result<()> {
        let Some(client) = self.clients.get(&ours) else { return Ok(()) };
        let target = client.window;
        if debug() {
            eprintln!("xim -> {} {:02x?}", message[0], &message[4..message.len().min(52)]);
        }
        let mut event = [0u8; 32];
        event[0] = x11::CLIENT_MESSAGE;
        event[4..8].copy_from_slice(&target.to_le_bytes());
        event[8..12].copy_from_slice(&self.atoms.protocol.to_le_bytes());
        if message.len() <= 20 {
            event[1] = 8;
            event[12..12 + message.len()].copy_from_slice(&message);
        } else {
            self.property_counter = (self.property_counter + 1) % 64;
            let property = self.x.atom(&format!("_minkeyboard_{}", self.property_counter))?;
            self.x.change_property(target, property, x11::ATOM_STRING, 8, x11::PROP_APPEND, &message)?;
            event[1] = 32;
            event[12..16].copy_from_slice(&(message.len() as u32).to_le_bytes());
            event[16..20].copy_from_slice(&property.to_le_bytes());
        }
        self.x.send_event(target, 0, &event)
    }

    fn ic(&mut self, focus: Focus) -> Option<&mut Ic> {
        self.clients.get_mut(&focus.client)?.ims.get_mut(&focus.im)?.ics.get_mut(&focus.ic)
    }

    fn handle_xim(&mut self, ours: u32, packet: &[u8]) -> io::Result<()> {
        let body = &packet[4..];
        if debug() {
            eprintln!("xim <- {} {:02x?}", packet[0], &body[..body.len().min(48)]);
        }
        let im = u16_at(body, 0);
        let ic = u16_at(body, 2);
        match packet[0] {
            CONNECT => {
                if body[0] != b'l' {
                    eprintln!("min-keyboard: a big-endian XIM client is not supported");
                }
                self.send(ours, message(CONNECT_REPLY, Body::default().u16(1).u16(0)))
            }
            DISCONNECT => {
                self.send(ours, message(DISCONNECT_REPLY, Body::default()))?;
                self.drop_client(ours)
            }
            OPEN => {
                let Some(client) = self.clients.get_mut(&ours) else { return Ok(()) };
                client.next_im = client.next_im.wrapping_add(1).max(1);
                let id = client.next_im;
                client.ims.insert(id, Im::default());
                let im_attributes = attribute_list(&[("queryInputStyle", T_STYLES)]);
                let ic_attributes = attribute_list(&IC_ATTRIBUTES);
                let body = Body::default()
                    .u16(id)
                    .u16(im_attributes.len() as u16)
                    .bytes(&im_attributes)
                    .u16(ic_attributes.len() as u16)
                    .u16(0)
                    .bytes(&ic_attributes);
                self.send(ours, message(OPEN_REPLY, body))
            }
            CLOSE => {
                if self.focus.is_some_and(|f| f.client == ours && f.im == im) {
                    self.focus = None;
                    self.session.clear();
                    self.hide()?;
                }
                if let Some(client) = self.clients.get_mut(&ours) {
                    client.ims.remove(&im);
                }
                self.send(ours, message(CLOSE_REPLY, Body::default().u16(im).u16(0)))
            }
            ENCODING_NEGOTIATION => {
                let len = u16_at(body, 2) as usize;
                let mut names = Vec::new();
                let mut at = 4;
                while at < 4 + len && at < body.len() {
                    let n = body[at] as usize;
                    names.push(String::from_utf8_lossy(body.get(at + 1..at + 1 + n).unwrap_or_default()).into_owned());
                    at += 1 + n;
                }
                // libX11 offers UTF-8 first but decodes committed text as COMPOUND_TEXT either
                // way, so take COMPOUND_TEXT whenever it is offered.
                let compound = names.iter().position(|n| n == "COMPOUND_TEXT");
                let utf8 = names.iter().position(|n| n.eq_ignore_ascii_case("UTF-8"));
                let index = compound.or(utf8).unwrap_or(0);
                if let Some(state) = self.clients.get_mut(&ours).and_then(|c| c.ims.get_mut(&im)) {
                    state.utf8 = compound.is_none() && utf8.is_some();
                }
                self.send(ours, message(ENCODING_NEGOTIATION_REPLY, Body::default().u16(im).u16(0).i16(index as i16).u16(0)))
            }
            QUERY_EXTENSION => self.send(ours, message(QUERY_EXTENSION_REPLY, Body::default().u16(im).u16(0))),
            GET_IM_VALUES => {
                let len = u16_at(body, 2) as usize;
                let mut values = Vec::new();
                for i in 0..len / 2 {
                    if u16_at(body, 4 + 2 * i) == 0 {
                        let mut styles = Body::default().u16(STYLES.len() as u16).u16(0);
                        for style in STYLES {
                            styles = styles.u32(style);
                        }
                        values.extend(attribute_value(0, &styles.0));
                    }
                }
                self.send(ours, message(GET_IM_VALUES_REPLY, Body::default().u16(im).u16(values.len() as u16).bytes(&values)))
            }
            SET_IM_VALUES => self.send(ours, message(SET_IM_VALUES_REPLY, Body::default().u16(im).u16(0))),
            CREATE_IC => {
                let len = u16_at(body, 2) as usize;
                let mut attributes = Vec::new();
                parse_attributes(body.get(4..4 + len).unwrap_or_default(), &mut attributes, 0);
                let Some(state) = self.clients.get_mut(&ours).and_then(|c| c.ims.get_mut(&im)) else { return Ok(()) };
                state.next_ic = state.next_ic.wrapping_add(1).max(1);
                let id = state.next_ic;
                let mut new_ic = Ic::default();
                new_ic.apply(&attributes);
                state.ics.insert(id, new_ic);
                self.send(ours, message(CREATE_IC_REPLY, Body::default().u16(im).u16(id)))?;
                // Forward key presses to us, asynchronously.
                self.send(ours, message(SET_EVENT_MASK, Body::default().u16(im).u16(id).u32(KEY_PRESS_MASK).u32(0)))
            }
            DESTROY_IC => {
                if self.focus == Some(Focus { client: ours, im, ic }) {
                    self.focus = None;
                    self.session.clear();
                    self.hide()?;
                }
                if let Some(state) = self.clients.get_mut(&ours).and_then(|c| c.ims.get_mut(&im)) {
                    state.ics.remove(&ic);
                }
                self.send(ours, message(DESTROY_IC_REPLY, Body::default().u16(im).u16(ic)))
            }
            SET_IC_VALUES => {
                let len = u16_at(body, 4) as usize;
                let mut attributes = Vec::new();
                parse_attributes(body.get(8..8 + len).unwrap_or_default(), &mut attributes, 0);
                let focus = Focus { client: ours, im, ic };
                if let Some(state) = self.ic(focus) {
                    state.apply(&attributes);
                }
                self.send(ours, message(SET_IC_VALUES_REPLY, Body::default().u16(im).u16(ic)))?;
                if self.focus == Some(focus) {
                    self.refresh_popup()?;
                }
                Ok(())
            }
            GET_IC_VALUES => {
                let len = u16_at(body, 4) as usize;
                let ids: Vec<u16> = (0..len / 2).map(|i| u16_at(body, 6 + 2 * i)).collect();
                let values = match self.ic(Focus { client: ours, im, ic }) {
                    Some(state) => ic_values(state, &ids),
                    None => Vec::new(),
                };
                self.send(ours, message(GET_IC_VALUES_REPLY, Body::default().u16(im).u16(ic).u16(values.len() as u16).u16(0).bytes(&values)))
            }
            SET_IC_FOCUS => {
                let focus = Focus { client: ours, im, ic };
                if self.focus != Some(focus) {
                    self.end_composition()?;
                    self.focus = Some(focus);
                }
                self.refresh_popup()
            }
            UNSET_IC_FOCUS => {
                if self.focus == Some(Focus { client: ours, im, ic }) {
                    self.end_composition()?;
                    self.focus = None;
                    self.hide()?;
                }
                Ok(())
            }
            FORWARD_EVENT => self.forward_event(ours, im, ic, body),
            SYNC => self.send(ours, message(SYNC_REPLY, Body::default().u16(im).u16(ic))),
            RESET_IC => {
                let mut text = String::new();
                if self.focus == Some(Focus { client: ours, im, ic }) {
                    let core = lock(&self.core);
                    text = self.session.finish(&core);
                }
                let utf8 = self.clients.get(&ours).and_then(|c| c.ims.get(&im)).is_some_and(|s| s.utf8);
                let encoded = encode_text(&text, utf8);
                self.send(ours, message(RESET_IC_REPLY, Body::default().u16(im).u16(ic).u16(encoded.len() as u16).bytes(&encoded)))?;
                self.refresh_popup()
            }
            TRIGGER_NOTIFY => self.send(ours, message(TRIGGER_NOTIFY_REPLY, Body::default().u16(im).u16(ic))),
            ERROR => {
                eprintln!("min-keyboard: XIM client reported error {}", u16_at(body, 6));
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn forward_event(&mut self, ours: u32, im: u16, ic: u16, body: &[u8]) -> io::Result<()> {
        let flag = u16_at(body, 4);
        let serial = u16_at(body, 6);
        let Some(event) = body.get(8..40) else { return Ok(()) };
        let focus = Focus { client: ours, im, ic };
        let mut handled = false;
        if event[0] & 0x7f == KEY_PRESS {
            if self.focus != Some(focus) {
                self.end_composition()?;
                self.focus = Some(focus);
            }
            let state = u32::from(u16_at(event, 28));
            let sym = self.keysym(event[1], state);
            let outcome = {
                let mut core = lock(&self.core);
                self.session.key(&mut core, sym, state)
            };
            if !outcome.commit.is_empty() {
                self.commit(focus, &outcome.commit)?;
            }
            if outcome.mode_changed {
                let core = lock(&self.core);
                let text = if core.enabled { core.language.label().to_string() } else { "Min Keyboard: off".to_string() };
                self.notice = Some((text, Instant::now() + Duration::from_millis(1200)));
            }
            handled = outcome.handled;
            self.refresh_popup()?;
        }
        if !handled {
            let back = Body::default().u16(im).u16(ic).u16(0).u16(serial).bytes(event);
            self.send(ours, message(FORWARD_EVENT, back))?;
        }
        if flag & XIM_SYNCHRONOUS != 0 {
            self.send(ours, message(SYNC_REPLY, Body::default().u16(im).u16(ic)))?;
        }
        Ok(())
    }

    fn keysym(&self, keycode: u8, state: u32) -> u32 {
        let (per, table) = &self.keymap;
        let index = keycode.saturating_sub(self.x.setup.min_keycode) as usize * per;
        let levels = table.get(index..index + per.min(&2)).unwrap_or_default();
        keysym::choose(levels, state)
    }

    fn commit(&mut self, focus: Focus, text: &str) -> io::Result<()> {
        let utf8 = self.clients.get(&focus.client).and_then(|c| c.ims.get(&focus.im)).is_some_and(|s| s.utf8);
        let encoded = encode_text(text, utf8);
        let body = Body::default().u16(focus.im).u16(focus.ic).u16(XIM_LOOKUP_CHARS).u16(encoded.len() as u16).bytes(&encoded);
        self.send(focus.client, message(COMMIT, body))
    }

    /// Commits what is being typed to the focused context, as focus leaves it.
    fn end_composition(&mut self) -> io::Result<()> {
        if !self.session.is_composing() {
            return Ok(());
        }
        let text = {
            let core = lock(&self.core);
            self.session.finish(&core)
        };
        match self.focus {
            Some(focus) if !text.is_empty() => self.commit(focus, &text),
            _ => Ok(()),
        }
    }

    fn hide(&mut self) -> io::Result<()> {
        self.popup.popup = None;
        if self.popup.mapped {
            self.popup.mapped = false;
            self.x.unmap_window(self.popup.id)?;
        }
        Ok(())
    }

    /// Redraws and places the candidate window for the focused context, or hides it.
    fn refresh_popup(&mut self) -> io::Result<()> {
        let Some(focus) = self.focus else { return self.hide() };
        let (view, notice) = {
            let core = lock(&self.core);
            (self.session.view(&core), self.notice.as_ref().map(|(text, _)| text.clone()))
        };
        let Some(popup) = popup::render(&mut self.fonts, &self.theme, &view, true, notice.as_deref()) else { return self.hide() };
        let Some(ic) = self.ic(focus) else { return self.hide() };
        let (window, spot) = (ic.window(), ic.spot);
        let line = self.fonts.line_height() as i32;
        let (x, y) = match spot {
            Some((sx, sy)) => {
                let (rx, ry) = self.x.to_root(window, sx, sy)?;
                (rx, ry + line / 4)
            }
            None => {
                let (_, height) = self.x.geometry(window)?;
                let (rx, ry) = self.x.to_root(window, 0, height.min(i16::MAX as u16) as i16)?;
                (rx, ry + 2)
            }
        };
        let (w, h) = (popup.canvas.width, popup.canvas.height);
        let (screen_w, screen_h) = (i32::from(self.x.setup.width), i32::from(self.x.setup.height));
        let x = x.min(screen_w - w as i32).max(0);
        let y = if y + h as i32 > screen_h { (y - h as i32 - line - line / 4).max(0) } else { y };
        if self.popup.geometry != (x, y, w, h) || !self.popup.mapped {
            self.x.configure(self.popup.id, x, y, w as u32, h as u32)?;
            self.popup.geometry = (x, y, w, h);
        }
        self.popup.popup = Some(popup);
        if !self.popup.mapped {
            self.popup.mapped = true;
            self.x.map_window(self.popup.id)?;
        }
        self.draw()
    }

    fn draw(&mut self) -> io::Result<()> {
        if let (Some(popup), true) = (&self.popup.popup, self.popup.mapped) {
            self.x.put_image(self.popup.id, self.popup.gc, popup.canvas.width, popup.canvas.height, &popup.canvas.pixels)?;
        }
        Ok(())
    }
}


/// The XICATTRIBUTE list answering XIM_GET_IC_VALUES. A nested-list attribute is followed by
/// the ids it asks for, up to the separator.
fn ic_values(ic: &Ic, ids: &[u16]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < ids.len() {
        let id = ids[i];
        i += 1;
        match IC_ATTRIBUTES.get(id as usize).map(|a| a.1) {
            Some(T_NEST) => {
                let mut nested = Vec::new();
                while i < ids.len() && ids[i] != IC_SEPARATOR {
                    nested.extend(attribute_value(ids[i], &ic.value(ids[i])));
                    i += 1;
                }
                i += 1;
                nested.extend(attribute_value(IC_SEPARATOR, &[]));
                out.extend(attribute_value(id, &nested));
            }
            Some(T_SEPARATOR) | None => {}
            Some(_) => out.extend(attribute_value(id, &ic.value(id))),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_messages() {
        let reply = message(CONNECT_REPLY, Body::default().u16(1).u16(0));
        assert_eq!(reply, vec![2, 0, 1, 0, 1, 0, 0, 0]);
        let commit = message(COMMIT, Body::default().u16(1).u16(2).u16(XIM_LOOKUP_CHARS).u16(3).bytes(b"abc"));
        assert_eq!(commit.len() % 4, 0);
        assert_eq!(u16_at(&commit, 2) as usize * 4, commit.len() - 4);
    }

    #[test]
    fn attribute_lists_are_aligned() {
        let list = attribute_list(&IC_ATTRIBUTES);
        let mut at = 0;
        let mut names = Vec::new();
        while at < list.len() {
            let n = u16_at(&list, at + 4) as usize;
            names.push(String::from_utf8(list[at + 6..at + 6 + n].to_vec()).unwrap());
            at += 6 + n + pad4(2 + n);
            assert_eq!(at % 2, 0);
        }
        assert_eq!(at, list.len());
        assert_eq!(names[14], "spotLocation");
        assert_eq!(list.len() % 4, 0);
    }

    #[test]
    fn parses_nested_attributes() {
        let spot = attribute_value(IC_SPOT_LOCATION, &[10, 0, 20, 0]);
        let mut nested = spot.clone();
        nested.extend(attribute_value(IC_SEPARATOR, &[]));
        let mut list = attribute_value(IC_INPUT_STYLE, &0x404u32.to_le_bytes());
        list.extend(attribute_value(IC_CLIENT_WINDOW, &0x1234u32.to_le_bytes()));
        list.extend(attribute_value(4, &nested));
        let mut attributes = Vec::new();
        parse_attributes(&list, &mut attributes, 0);
        let mut ic = Ic::default();
        ic.apply(&attributes);
        assert_eq!(ic.style, 0x404);
        assert_eq!(ic.client_window, 0x1234);
        assert_eq!(ic.spot, Some((10, 20)));
        assert_eq!(ic.window(), 0x1234);
    }

    #[test]
    fn answers_ic_values() {
        let ic = Ic { style: 0x404, client_window: 7, focus_window: 9, spot: Some((1, 2)) };
        let values = ic_values(&ic, &[IC_FILTER_EVENTS, 4, IC_SPOT_LOCATION, IC_SEPARATOR, IC_INPUT_STYLE]);
        let mut parsed = Vec::new();
        parse_attributes(&values, &mut parsed, 0);
        assert_eq!(parsed[0], (IC_FILTER_EVENTS, KEY_PRESS_MASK.to_le_bytes().to_vec()));
        assert_eq!(parsed[1], (IC_SPOT_LOCATION, vec![1, 0, 2, 0]));
        assert_eq!(parsed.last().unwrap(), &(IC_INPUT_STYLE, 0x404u32.to_le_bytes().to_vec()));
    }

    #[test]
    fn encodes_text() {
        assert_eq!(encode_text("abc", false), b"abc");
        assert_eq!(encode_text("你好", false), [b"\x1b%G".as_slice(), "你好".as_bytes()].concat());
        assert_eq!(encode_text("你好", true), "你好".as_bytes());
        assert!(locales().starts_with("@locale=C,POSIX,"));
        assert!(locales().contains(",zh_CN,"));
    }
}
