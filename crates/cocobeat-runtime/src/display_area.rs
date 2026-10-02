use std::time::Duration;

use winit::{monitor::MonitorHandle, window::Window};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PhysicalRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[cfg(any(target_os = "linux", test))]
impl PhysicalRect {
    pub(crate) fn intersection(self, other: Self) -> Option<Self> {
        let left = i64::from(self.x.max(other.x));
        let top = i64::from(self.y.max(other.y));
        let right = (i64::from(self.x) + i64::from(self.width))
            .min(i64::from(other.x) + i64::from(other.width));
        let bottom = (i64::from(self.y) + i64::from(self.height))
            .min(i64::from(other.y) + i64::from(other.height));
        if left >= right || top >= bottom {
            return None;
        }
        Some(Self {
            x: i32::try_from(left).ok()?,
            y: i32::try_from(top).ok()?,
            width: u32::try_from(right - left).ok()?,
            height: u32::try_from(bottom - top).ok()?,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AreaSource {
    #[cfg_attr(not(windows), allow(dead_code))]
    Windows,
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    X11,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct AreaInfo {
    pub monitor: PhysicalRect,
    pub work_area: Option<PhysicalRect>,
    pub source: AreaSource,
    pub resize_allowed: Option<bool>,
}

impl AreaInfo {
    fn unknown(monitor: PhysicalRect) -> Self {
        Self {
            monitor,
            work_area: None,
            source: AreaSource::Unknown,
            resize_allowed: None,
        }
    }
}

#[derive(Default)]
pub(crate) struct AreaReader {
    cached: Option<(winit::window::WindowId, Duration, AreaInfo)>,
    #[cfg(target_os = "linux")]
    x11: Option<x11::Reader>,
}

impl AreaReader {
    pub(crate) fn observe(
        &mut self,
        window: &Window,
        monitor: &MonitorHandle,
        now: Duration,
    ) -> AreaInfo {
        let position = monitor.position();
        let size = monitor.size();
        let monitor = PhysicalRect {
            x: position.x,
            y: position.y,
            width: size.width,
            height: size.height,
        };
        // Poll property changes at most twice a second, refresh immediately on another output
        if let Some((id, at, info)) = self.cached
            && id == window.id()
            && info.monitor == monitor
            && now
                .checked_sub(at)
                .is_some_and(|age| age < Duration::from_millis(500))
        {
            return info;
        }
        let mut info = AreaInfo::unknown(monitor);
        #[cfg(windows)]
        if let Some(work_area) = windows_area(monitor) {
            info.work_area = Some(work_area);
            info.source = AreaSource::Windows;
        }
        #[cfg(target_os = "linux")]
        {
            use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

            let xid = window
                .window_handle()
                .ok()
                .and_then(|handle| match handle.as_raw() {
                    RawWindowHandle::Xlib(handle) => u32::try_from(handle.window).ok(),
                    RawWindowHandle::Xcb(handle) => Some(handle.window.get()),
                    _ => None,
                });
            if let Some(xid) = xid.filter(|&id| id != 0) {
                if self.x11.is_none() {
                    self.x11 = x11::Reader::connect();
                }
                if let Some(reader) = &self.x11 {
                    info = reader.observe(xid, monitor);
                }
            }
        }
        self.cached = Some((window.id(), now, info));
        info
    }
}

#[cfg(windows)]
fn windows_area(monitor: PhysicalRect) -> Option<PhysicalRect> {
    use winsafe::{HMONITOR, RECT, co};

    if monitor.width == 0 || monitor.height == 0 {
        return None;
    }
    let expected = RECT {
        left: monitor.x,
        top: monitor.y,
        right: monitor.x.checked_add_unsigned(monitor.width)?,
        bottom: monitor.y.checked_add_unsigned(monitor.height)?,
    };
    let info = HMONITOR::MonitorFromRect(expected, co::MONITOR::DEFAULTTONULL)
        .GetMonitorInfo()
        .ok()?;
    let work = info.rcWork;
    if info.rcMonitor != expected
        || work.left < expected.left
        || work.top < expected.top
        || work.right > expected.right
        || work.bottom > expected.bottom
        || work.left >= work.right
        || work.top >= work.bottom
    {
        return None;
    }
    Some(PhysicalRect {
        x: work.left,
        y: work.top,
        width: work.right.abs_diff(work.left),
        height: work.bottom.abs_diff(work.top),
    })
}

#[cfg(target_os = "linux")]
mod x11 {
    use super::{AreaInfo, AreaSource, PhysicalRect};
    use x11rb::{
        connection::Connection,
        protocol::xproto::{Atom, AtomEnum, ConnectionExt, GetPropertyReply},
        rust_connection::RustConnection,
    };

    x11rb::atom_manager! {
        Atoms: AtomsCookie {
            _NET_SUPPORTED,
            _NET_SUPPORTING_WM_CHECK,
            _NET_NUMBER_OF_DESKTOPS,
            _NET_CURRENT_DESKTOP,
            _NET_WM_DESKTOP,
            _NET_DESKTOP_VIEWPORT,
            _NET_WORKAREA,
            _NET_WM_ALLOWED_ACTIONS,
            _NET_WM_ACTION_RESIZE,
        }
    }

    pub(super) struct Reader {
        connection: RustConnection,
        atoms: Atoms,
    }

    impl Reader {
        pub(super) fn connect() -> Option<Self> {
            let (connection, _) = x11rb::connect(None).ok()?;
            let atoms = Atoms::new(&connection).ok()?.reply().ok()?;
            Some(Self { connection, atoms })
        }

        fn property(
            &self,
            window: u32,
            atom: Atom,
            kind: AtomEnum,
        ) -> Result<Option<Vec<u32>>, ()> {
            let reply = self
                .connection
                .get_property(false, window, atom, kind, 0, 4_096)
                .map_err(|_| ())?
                .reply()
                .map_err(|_| ())?;
            property_words(&reply, kind.into())
        }

        pub(super) fn observe(&self, window: u32, monitor: PhysicalRect) -> AreaInfo {
            let mut info = AreaInfo::unknown(monitor);
            let Some((root, supported)) = self.wm(window) else {
                return info;
            };
            let atoms = &self.atoms;
            if supported.contains(&atoms._NET_WM_ALLOWED_ACTIONS) {
                info.resize_allowed = self
                    .property(window, atoms._NET_WM_ALLOWED_ACTIONS, AtomEnum::ATOM)
                    .ok()
                    .flatten()
                    .map(|actions| actions.contains(&atoms._NET_WM_ACTION_RESIZE));
            }
            let read_area = || -> Option<PhysicalRect> {
                if ![
                    atoms._NET_NUMBER_OF_DESKTOPS,
                    atoms._NET_CURRENT_DESKTOP,
                    atoms._NET_DESKTOP_VIEWPORT,
                    atoms._NET_WORKAREA,
                ]
                .iter()
                .all(|atom| supported.contains(atom))
                {
                    return None;
                }
                let cardinal = |window, atom| self.property(window, atom, AtomEnum::CARDINAL);
                let count = cardinal(root, atoms._NET_NUMBER_OF_DESKTOPS).ok()??;
                let current = cardinal(root, atoms._NET_CURRENT_DESKTOP).ok()??;
                let desktop = cardinal(window, atoms._NET_WM_DESKTOP).ok()?;
                let viewports = cardinal(root, atoms._NET_DESKTOP_VIEWPORT).ok()??;
                let areas = cardinal(root, atoms._NET_WORKAREA).ok()??;
                // Desktop changes during these reads cannot produce a trusted mixed snapshot
                if cardinal(root, atoms._NET_CURRENT_DESKTOP).ok()?? != current {
                    return None;
                }
                desktop_area(
                    monitor,
                    &count,
                    &current,
                    desktop.as_deref(),
                    &viewports,
                    &areas,
                )
            };
            if let Some(area) = read_area() {
                info.work_area = Some(area);
                info.source = AreaSource::X11;
            }
            info
        }

        fn wm(&self, window: u32) -> Option<(u32, Vec<u32>)> {
            let root = self
                .connection
                .get_geometry(window)
                .ok()?
                .reply()
                .ok()?
                .root;
            // Resolve the window's own root, not connect()'s default screen
            if !self
                .connection
                .setup()
                .roots
                .iter()
                .any(|screen| screen.root == root)
            {
                return None;
            }
            let atom = self.atoms._NET_SUPPORTING_WM_CHECK;
            let check = self.property(root, atom, AtomEnum::WINDOW).ok()??;
            let [check] = check.as_slice() else {
                return None;
            };
            if *check == 0 || self.property(*check, atom, AtomEnum::WINDOW).ok()?? != [*check] {
                return None;
            }
            let supported = self
                .property(root, self.atoms._NET_SUPPORTED, AtomEnum::ATOM)
                .ok()??;
            Some((root, supported))
        }
    }

    fn property_words(reply: &GetPropertyReply, kind: Atom) -> Result<Option<Vec<u32>>, ()> {
        if reply.type_ == u32::from(AtomEnum::NONE) && reply.format == 0 && reply.bytes_after == 0 {
            return if reply.value_len == 0 && reply.value.is_empty() {
                Ok(None)
            } else {
                Err(())
            };
        }
        if reply.type_ != kind || reply.format != 32 || reply.bytes_after != 0 {
            return Err(());
        }
        let words: Vec<_> = reply.value32().ok_or(())?.collect();
        if words.len() != reply.value_len as usize || reply.value.len() != words.len() * 4 {
            return Err(());
        }
        Ok(Some(words))
    }

    fn desktop_area(
        monitor: PhysicalRect,
        count: &[u32],
        current: &[u32],
        desktop: Option<&[u32]>,
        viewports: &[u32],
        areas: &[u32],
    ) -> Option<PhysicalRect> {
        let [count] = *count else { return None };
        let [current] = *current else { return None };
        if count == 0 || count > 1_024 || current >= count {
            return None;
        }
        let selected = match desktop {
            None | Some([u32::MAX]) => current,
            Some([index]) if *index < count => *index,
            _ => return None,
        } as usize;
        if viewports.len() != count as usize * 2 || areas.len() != count as usize * 4 {
            return None;
        }
        // EWMH work areas are viewport-relative, already in the monitor's root coordinate frame
        // Validate viewport coordinates without adding their virtual-desktop origin a second time
        for &coordinate in &viewports[selected * 2..selected * 2 + 2] {
            i32::try_from(coordinate).ok()?;
        }
        let area = &areas[selected * 4..selected * 4 + 4];
        monitor.intersection(PhysicalRect {
            x: i32::try_from(area[0]).ok()?,
            y: i32::try_from(area[1]).ok()?,
            width: area[2],
            height: area[3],
        })
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn ewmh_values_validate_desktops_viewports_and_partial_properties() {
            let monitor = PhysicalRect {
                x: 1920,
                y: 0,
                width: 1920,
                height: 1080,
            };
            let areas = [0, 40, 3840, 1040, 0, 0, 3840, 1000];
            let viewports = [1920, 0, 0, 0];
            let area = |count, current, desktop, viewports: &[u32], areas: &[u32]| {
                desktop_area(monitor, count, current, desktop, viewports, areas)
            };
            assert_eq!(
                area(&[2], &[0], None, &viewports, &areas),
                Some(PhysicalRect {
                    x: 1920,
                    y: 40,
                    width: 1920,
                    height: 1040,
                })
            );
            assert_eq!(
                area(&[2], &[0], Some(&[1]), &viewports, &areas)
                    .unwrap()
                    .height,
                1000
            );
            assert_eq!(
                area(&[2], &[1], Some(&[u32::MAX]), &viewports, &areas)
                    .unwrap()
                    .height,
                1000
            );
            for (count, current, desktop, viewport, work) in [
                (&[0][..], &[0][..], None, &viewports[..], &areas[..]),
                (&[2], &[2], None, &viewports, &areas),
                (&[2], &[0], Some(&[2][..]), &viewports, &areas),
                (&[2], &[0], Some(&[][..]), &viewports, &areas),
                (&[2], &[0], None, &viewports[..2], &areas),
                (&[2], &[0], None, &viewports, &areas[..4]),
                (&[2], &[0], None, &[u32::MAX, 0, 0, 0], &areas),
            ] {
                assert!(area(count, current, desktop, viewport, work).is_none());
            }
            let mut reply = GetPropertyReply {
                format: 32,
                type_: AtomEnum::ATOM.into(),
                value_len: 0,
                ..Default::default()
            };
            assert_eq!(
                property_words(&reply, AtomEnum::ATOM.into()),
                Ok(Some(vec![]))
            );
            reply.bytes_after = 4;
            assert!(property_words(&reply, AtomEnum::ATOM.into()).is_err());
            reply.bytes_after = 0;
            assert!(property_words(&reply, AtomEnum::CARDINAL.into()).is_err());
            reply.value_len = 1;
            assert!(property_words(&reply, AtomEnum::ATOM.into()).is_err());
            assert_eq!(
                property_words(&GetPropertyReply::default(), AtomEnum::ATOM.into()),
                Ok(None)
            );
            assert!(
                property_words(
                    &GetPropertyReply {
                        value_len: 1,
                        ..Default::default()
                    },
                    AtomEnum::ATOM.into(),
                )
                .is_err()
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn physical_intersections_preserve_negative_origins_and_reject_empty_areas() {
        let monitor = PhysicalRect {
            x: -1920,
            y: -100,
            width: 1920,
            height: 1080,
        };
        let work = PhysicalRect {
            x: -3840,
            y: -60,
            width: 3840,
            height: 1040,
        };
        assert_eq!(
            monitor.intersection(work),
            Some(PhysicalRect {
                x: -1920,
                y: -60,
                width: 1920,
                height: 1040,
            })
        );
        assert!(
            monitor
                .intersection(PhysicalRect { x: 0, ..monitor })
                .is_none()
        );
        assert!(
            monitor
                .intersection(PhysicalRect {
                    width: 0,
                    ..monitor
                })
                .is_none()
        );
        let large = PhysicalRect {
            x: i32::MAX,
            y: 0,
            width: u32::MAX,
            height: 1,
        };
        assert_eq!(large.intersection(large), Some(large));
    }
}
