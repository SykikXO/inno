// Moves the pointer to an absolute position and clicks, through the wlr virtual
// pointer protocol.
//
// This exists because testing anything about inno's hit testing needs real
// pointer events aimed at exact pixels, and there was no way to produce them:
// ydotool needs a root daemon that is not installed, and synthesising events
// from inside the daemon under test would be testing the test. A virtual pointer
// is a plain Wayland client, so it needs no privileges, no daemon and no new
// crates: the protocol already ships as a transitive dependency.
//
//   cargo run --example click -- X Y
//   cargo run --example click -- X Y EX EY
//
// EX/EY default to the output's logical size, which is the coordinate frame
// motion_absolute is expressed in. Pass them explicitly when the position was
// derived on a different display than the one being clicked.

use wayland_client::protocol::{wl_output, wl_pointer, wl_registry};
use wayland_client::{Connection, Dispatch, QueueHandle, delegate_noop};
use wayland_protocols_wlr::virtual_pointer::v1::client::{
    zwlr_virtual_pointer_manager_v1, zwlr_virtual_pointer_v1,
};

/// BTN_LEFT from linux/input-event-codes.h.
const BTN_LEFT: u32 = 0x110;

struct Compositor {
    manager: Option<zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1>,
    size: Option<(u32, u32)>,
}

impl Dispatch<wl_registry::WlRegistry, ()> for Compositor {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        (): &(),
        _conn: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        let wl_registry::Event::Global { name, interface, version } = event else { return };
        if interface == "zwlr_virtual_pointer_manager_v1" && state.manager.is_none() {
            // version 2 only adds create_virtual_pointer_with_output, which
            // needs an output handle. With one output the compositor picks it
            // anyway, so v1 is enough.
            state.manager = Some(registry.bind(name, version.min(1), qh, ()));
        } else if interface == "wl_output" && state.size.is_none() {
            let _output: wl_output::WlOutput = registry.bind(name, version.min(4), qh, ());
        }
    }
}

impl Dispatch<wl_output::WlOutput, ()> for Compositor {
    fn event(
        state: &mut Self,
        _output: &wl_output::WlOutput,
        event: wl_output::Event,
        (): &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        // Geometry reports physical millimetres, which is useless for this. The
        // logical size arrives as Mode, before Done.
        if let wl_output::Event::Mode { width, height, .. } = event {
            state.size = Some((width.max(0) as u32, height.max(0) as u32));
        }
    }
}

delegate_noop!(Compositor: zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1);
delegate_noop!(Compositor: zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1);

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let num = |i: usize, what: &str| -> anyhow::Result<u32> {
        args.get(i)
            .ok_or_else(|| anyhow::anyhow!("missing {what}"))?
            .parse::<i64>()
            .map(|v| v.max(0) as u32)
            .map_err(|_| anyhow::anyhow!("{what} must be a number, got {:?}", args[i]))
    };
    let x = num(0, "X")?;
    let y = num(1, "Y")?;

    let conn = Connection::connect_to_env()?;
    let mut queue = conn.new_event_queue::<Compositor>();
    let qh = queue.handle();
    let _registry = conn.display().get_registry(&qh, ());
    let mut state = Compositor { manager: None, size: None };

    // Two roundtrips: the first delivers the globals, the second the Mode event
    // that binding wl_output asks for.
    let _ = queue.roundtrip(&mut state);
    let _ = queue.roundtrip(&mut state);

    let manager = state.manager.as_ref().ok_or_else(|| {
        anyhow::anyhow!("compositor does not implement zwlr_virtual_pointer_manager_v1")
    })?;
    let (ex, ey) = match (args.get(2), args.get(3)) {
        (Some(_), Some(_)) => (num(2, "EX")?, num(3, "EY")?),
        _ => state
            .size
            .ok_or_else(|| anyhow::anyhow!("no output reported a mode to size the pointer in"))?,
    };

    let pointer = manager.create_virtual_pointer(None, &qh, ());

    pointer.motion_absolute(0, x, y, ex, ey);
    pointer.frame();
    let _ = queue.roundtrip(&mut state);

    // A press and release the compositor never saw as two events is not a click.
    // The pause before it is not optional either: every virtual pointer is a new
    // device, and pressing in the same breath as the motion loses the race
    // against the compositor working out that the pointer is now over the
    // surface. The press lands on whatever had focus before, and is silently
    // dropped.
    std::thread::sleep(std::time::Duration::from_millis(150));
    // Move again so the compositor has an enter to act on, then wait for it.
    pointer.motion_absolute(0, x, y, ex, ey);
    pointer.frame();
    let _ = queue.roundtrip(&mut state);
    std::thread::sleep(std::time::Duration::from_millis(150));

    pointer.button(0, BTN_LEFT, wl_pointer::ButtonState::Pressed);
    pointer.frame();
    let _ = queue.roundtrip(&mut state);
    std::thread::sleep(std::time::Duration::from_millis(60));
    pointer.button(0, BTN_LEFT, wl_pointer::ButtonState::Released);
    pointer.frame();
    let _ = queue.roundtrip(&mut state);

    println!("clicked at {x},{y} in a {ex}x{ey} frame");
    Ok(())
}
