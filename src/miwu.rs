//! Multi-Input Wake-Up (MIWU) control for exiting power states, and signal conditioning of external interrupt sources.
//!
//! # Realtime `RT` feature
//! If the `rt` feature is enabled, provides an interface to `await` on an [WakeUpInput] using [WakeUp].
//! Without this feature enabled, the [WakeUpInput] can still be enabled and checked whether it [is high](WakeUp::is_high)
//! or [is pending](WakeUp::is_pending).
//!
//! The interrupts need to be unmasked in `NVIC` in order for this functionality to be used.
//!
//! ## Opinionated interrupt
//! The interrupts implemented here will unset the `enable` bit, but leave the `pending` bit intact. It is the future
//! that clears this `pending` bit when polled or dropped.
//!
//! This means that if the interrupt is run, all pending WakeUpInputs are disabled, and need to be re-enabled if used for
//! exiting a low power state.
//!
//! # Use cases
//! * View [AwaitableInput](crate::gpio_miwu::AwaitableInput) (if `rt` feature is enabled) to configure an pin interrupt.
//! * These WakeUpInputs can be consumed by the HAL implementation for specific peripherals unrelated to GPIO pins.

use embassy_hal_internal::{into_ref, Peripheral, PeripheralRef};
use paste::paste;

const MIWU_COUNT: usize = 3;
const SUBGROUP_COUNT: usize = 8;
const GROUP_COUNT: usize = 8;
const WUI_COUNT: usize = MIWU_COUNT * GROUP_COUNT * SUBGROUP_COUNT;

/// Index used to access array elements (used for AtomicWakers) or to store AnyWakeUpInput compactly.
#[derive(Clone, Copy)]
struct WuiIndex(u8);

/// Expanded WuiIndex used to meaningfully access registers and their bits.
#[derive(Clone)]
struct WuiMap {
    pub miwu_n: u8,
    pub group: u8,
    pub subgroup: u8,
}

impl WuiMap {
    pub const fn port(&self) -> &'static crate::pac::miwu0::RegisterBlock {
        get_miwu(self.miwu_n as usize)
    }
}

const fn div_rem(x: u8, y: u8) -> (u8, u8) {
    (x / y, x % y)
}

impl WuiIndex {
    pub const fn new(map: WuiMap) -> Self {
        let i = map.miwu_n * SUBGROUP_COUNT as u8 * GROUP_COUNT as u8 + map.group * SUBGROUP_COUNT as u8 + map.subgroup;

        assert!(i < WUI_COUNT as u8);

        Self(i)
    }

    pub const fn to_map(self) -> WuiMap {
        let (r, subgroup) = div_rem(self.0, SUBGROUP_COUNT as u8);
        let (miwu_n, group) = div_rem(r, GROUP_COUNT as u8);

        assert!(miwu_n < MIWU_COUNT as u8);

        WuiMap {
            miwu_n,
            group,
            subgroup,
        }
    }
}

const fn get_miwu(n: usize) -> &'static crate::pac::miwu0::RegisterBlock {
    const MIWU_N: [*const crate::pac::miwu0::RegisterBlock; MIWU_COUNT] = [
        crate::pac::Miwu0::ptr(),
        crate::pac::Miwu1::ptr(),
        crate::pac::Miwu2::ptr(),
    ];

    let ptr = MIWU_N[n];
    // Safety:
    // the pac ptr functions return pointers to memory that is used for registers for the 'static lifetime
    // and the created reference is shared.
    unsafe { &*ptr }
}

/// Signal level used as signalling condition.
pub enum Level {
    Low,
    High,
}

/// Signal edge used as signalling condition.
pub enum Edge {
    Any,
    Falling,
    Rising,
}

/// Signalling condition on which the [WakeUp] input is triggered.
pub enum Mode {
    Level(Level),
    Edge(Edge),
}

impl From<Level> for Mode {
    fn from(value: Level) -> Self {
        Mode::Level(value)
    }
}

impl From<Edge> for Mode {
    fn from(value: Edge) -> Self {
        Mode::Edge(value)
    }
}

mod sealed {
    pub trait SealedWakeUpInput {
        #![allow(private_interfaces)]
        fn as_map(&self) -> super::WuiMap;
    }
}

/// WakeUpInput (WUI) trait.
pub trait WakeUpInput: sealed::SealedWakeUpInput {}

/// WakeUpInput (WUI) driver.
pub struct WakeUp<'d> {
    wui: PeripheralRef<'d, AnyWakeUpInput>,
}

impl<'d> WakeUp<'d> {
    /// Construct the WakeUp driver without enabling the signalling condition.
    pub fn new(wui: impl Peripheral<P = impl WakeUpInput + 'd> + 'd) -> Self {
        into_ref!(wui);
        Self { wui: wui.map_into() }
    }

    fn as_map(&self) -> WuiMap {
        self.wui.0.to_map()
    }

    /// Enable the [WakeUpInput] with a specific signalling condition [Mode], enabling triggering the WakeUp signal and/or interrupt.
    pub fn enable(&mut self, mode: impl Into<Mode>) {
        let map = self.as_map();
        let port = map.port();
        let group = map.group as usize;

        use crate::pac::miwu0::*;
        let (wkmod, wkaedgn, wkedgn);
        match mode.into() {
            Mode::Level(level) => {
                wkmod = wkmodn::InputMode::Level;
                wkaedgn = None;
                wkedgn = Some(match level {
                    Level::Low => wkedgn::Edge::LowFalling,
                    Level::High => wkedgn::Edge::HighRising,
                });
            }
            Mode::Edge(edge) => {
                wkmod = wkmodn::InputMode::Edge;
                (wkaedgn, wkedgn) = match edge {
                    Edge::Any => (Some(wkaedgn::AnyEdge::Any), None),
                    Edge::Falling => (Some(wkaedgn::AnyEdge::Edge), Some(wkedgn::Edge::LowFalling)),
                    Edge::Rising => (Some(wkaedgn::AnyEdge::Edge), Some(wkedgn::Edge::HighRising)),
                };
            }
        }

        // Note(cs): WakeUpInputs can share MIWU and group, which use the same registers.
        critical_section::with(|_cs| {
            port.wkenn(group).modify(|_, w| w.input(map.subgroup).disabled());
            port.wkmodn(group).modify(|_, w| w.input(map.subgroup).variant(wkmod));

            if let Some(wkaedgn) = wkaedgn {
                port.wkaedgn(group)
                    .modify(|_, w| w.input(map.subgroup).variant(wkaedgn));
            }

            if let Some(wkedgn) = wkedgn {
                port.wkedgn(group).modify(|_, w| w.input(map.subgroup).variant(wkedgn));
            }

            port.wkinenn(group).modify(|_, w| w.input(map.subgroup).enabled());
            port.wkpcln(group).write(|w| w.input(map.subgroup).clear());
            port.wkenn(group).modify(|_, w| w.input(map.subgroup).enabled());
        });
    }

    /// Disable the [WakeUpInput], forbidding the WakeUp signal and/or interrupt.
    pub fn disable(&mut self) {
        let map = self.as_map();
        // Note(cs): WakeUpInputs can share MIWU and group, which use the same registers.
        critical_section::with(|_cs| {
            map.port()
                .wkenn(map.group as usize)
                .modify(|_, w| w.input(map.subgroup).disabled());
        });
    }

    pub fn clear_pending(&mut self) {
        let map = self.as_map();
        // Note(no-cs): atomic write to clear a single bit, safe.
        map.port()
            .wkpcln(map.group as usize)
            .write(|w| w.input(map.subgroup).clear());
    }

    /// Indicates whether the input signal, regardless of signalling condition, is high or not.
    pub fn is_high(&self) -> bool {
        let map = self.as_map();
        map.port()
            .wkstn(map.group as usize)
            .read()
            .input(map.subgroup)
            .is_high()
    }

    /// Indicates whether the input signalling condition set in [Mode] (example: rising edge) has been triggered.
    pub fn is_pending(&self) -> bool {
        let map = self.as_map();
        map.port()
            .wkpndn(map.group as usize)
            .read()
            .input(map.subgroup)
            .is_pending()
    }
}

/// Disables the [WakeUpInput] signalling condition when dropped.
impl Drop for WakeUp<'_> {
    fn drop(&mut self) {
        self.disable();
    }
}

struct AnyWakeUpInput(WuiIndex);

// Allow use of PeripheralRef to do lifetime management
impl Peripheral for AnyWakeUpInput {
    type P = AnyWakeUpInput;

    unsafe fn clone_unchecked(&self) -> Self::P {
        AnyWakeUpInput(self.0)
    }
}

impl<T: WakeUpInput> From<T> for AnyWakeUpInput {
    fn from(value: T) -> Self {
        AnyWakeUpInput(WuiIndex::new(value.as_map()))
    }
}

macro_rules! impl_wake_up_input {
    ($peripheral:ty, $miwu_n:expr, $group:expr, $subgroup:expr, $interrupt:ident) => {
        impl sealed::SealedWakeUpInput for $peripheral {
            #![allow(private_interfaces)]
            fn as_map(&self) -> self::WuiMap {
                self::WuiMap {
                    miwu_n: $miwu_n,
                    group: $group,
                    subgroup: $subgroup,
                }
            }
        }
        impl WakeUpInput for $peripheral {}
    };
}

#[cfg(feature = "rt")]
/// Interrupt handling for MIWU, enabling to `await` on [WakeUp] signalling conditions.
mod rt {
    use core::future::Future;
    use core::task::{Context, Poll};

    use embassy_sync::waitqueue::AtomicWaker;

    use super::*;
    use crate::pac::interrupt;

    // Note: having 192 wakers costs quite a bit of RAM.
    // If desired, change to or add intrusive linked list waker to save RAM.
    static MIWU_WAKERS: [AtomicWaker; WUI_COUNT] = [const { AtomicWaker::new() }; WUI_COUNT];

    const fn get_waker(map: WuiMap) -> &'static AtomicWaker {
        &MIWU_WAKERS[WuiIndex::new(map).0 as usize]
    }

    impl<'d> WakeUp<'d> {
        /// Configures a specific signalling condition [Mode] and awaits for it to be signalled.
        pub async fn wait_for(&mut self, mode: impl Into<Mode>) {
            self.enable(mode);
            WakeUpInputFuture::<'_, 'd> { channel: self }.await
        }

        /// Configures the [Level::High] signalling condition and awaits for it to be signalled.
        pub async fn wait_for_high(&mut self) {
            self.wait_for(Level::High).await
        }

        /// Configures the [Level::Low] signalling condition and awaits for it to be signalled.
        pub async fn wait_for_low(&mut self) {
            self.wait_for(Level::Low).await
        }

        fn waker(&self) -> &'static AtomicWaker {
            get_waker(self.as_map())
        }
    }

    struct WakeUpInputFuture<'a, 'd> {
        channel: &'a mut WakeUp<'d>,
    }

    impl Drop for WakeUpInputFuture<'_, '_> {
        fn drop(&mut self) {
            // Clean up, and do not assume that the interrupt has run.
            self.channel.disable();
            self.channel.clear_pending();
        }
    }

    impl Future for WakeUpInputFuture<'_, '_> {
        type Output = ();

        fn poll(self: core::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            self.channel.waker().register(cx.waker());

            if self.channel.is_pending() {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        }
    }

    struct BitIter(u8);

    impl Iterator for BitIter {
        type Item = u8;

        fn next(&mut self) -> Option<Self::Item> {
            match self.0.trailing_zeros() {
                8 => None,
                b => {
                    self.0 &= !(1 << b);
                    Some(b as u8)
                }
            }
        }
    }

    fn on_irq(miwu_n: usize, group: usize) {
        let port = get_miwu(miwu_n);

        let pending = port.wkpndn(group).read();
        for subgroup in BitIter(pending.bits()) {
            let waker = get_waker(WuiMap {
                miwu_n: miwu_n as u8,
                group: group as u8,
                subgroup,
            });
            waker.wake();
        }

        critical_section::with(|_cs| {
            port.wkenn(group)
                .modify(|r, w| unsafe { w.bits(r.bits() & !pending.bits()) });
        });
    }

    macro_rules! impl_irq {
        ($interrupt:ident, $miwu_n:literal, $group:literal) => {
            #[allow(non_snake_case)]
            #[interrupt]
            unsafe fn $interrupt() {
                on_irq($miwu_n, $group - 1) // The groups are 1-indexed
            }
        };
    }

    impl_irq!(WKINTA_0, 0, 1);
    impl_irq!(WKINTB_0, 0, 2);
    impl_irq!(WKINTC_0, 0, 3);
    impl_irq!(WKINTD_0, 0, 4);
    impl_irq!(WKINTE_0, 0, 5);
    impl_irq!(WKINTF_0, 0, 6);
    impl_irq!(WKINTG_0, 0, 7);
    impl_irq!(WKINTH_0, 0, 8);
    impl_irq!(WKINTA_1, 1, 1);
    impl_irq!(WKINTB_1, 1, 2);
    impl_irq!(WKINTC_1, 1, 3);
    impl_irq!(WKINTD_1, 1, 4);
    impl_irq!(WKINTE_1, 1, 5);
    impl_irq!(WKINTF_1, 1, 6);
    impl_irq!(WKINTG_1, 1, 7);
    impl_irq!(WKINTH_1, 1, 8);
    impl_irq!(WKINTA_2, 2, 1);
    impl_irq!(WKINTB_2, 2, 2);
    impl_irq!(WKINTC_2, 2, 3);
    impl_irq!(WKINTD_2, 2, 4);
    impl_irq!(WKINTE_2, 2, 5);
    impl_irq!(WKINTF_2, 2, 6);
    impl_irq!(WKINTG_2, 2, 7);
    impl_irq!(WKINTH_2, 2, 8);
}

macro_rules! impl_wake_up_input_n {
    ($miwu_n:literal, $group:literal, $subgroup:literal, $interrupt:ident) => {
        paste! { impl_wake_up_input!(
                crate::peripherals::[<MIWU $miwu_n _ $group $subgroup>],
                $miwu_n,
                ($group - 1), // The groups are 1-indexed
                $subgroup,
                $interrupt
            );
        }
    };
}

macro_rules! impl_wake_up_input_nm {
    ($miwu_n:literal, $group:literal, $interrupt:ident) => {
        impl_wake_up_input_n!($miwu_n, $group, 0, $interrupt);
        impl_wake_up_input_n!($miwu_n, $group, 1, $interrupt);
        impl_wake_up_input_n!($miwu_n, $group, 2, $interrupt);
        impl_wake_up_input_n!($miwu_n, $group, 3, $interrupt);
        impl_wake_up_input_n!($miwu_n, $group, 4, $interrupt);
        impl_wake_up_input_n!($miwu_n, $group, 5, $interrupt);
        impl_wake_up_input_n!($miwu_n, $group, 6, $interrupt);
        impl_wake_up_input_n!($miwu_n, $group, 7, $interrupt);
    };
}

impl_wake_up_input_nm!(0, 1, WKINTA_0);
impl_wake_up_input_nm!(0, 2, WKINTB_0);
impl_wake_up_input_nm!(0, 3, WKINTC_0);
impl_wake_up_input_nm!(0, 4, WKINTD_0);
impl_wake_up_input_nm!(0, 5, WKINTE_0);
impl_wake_up_input_nm!(0, 6, WKINTF_0);
impl_wake_up_input_nm!(0, 7, WKINTG_0);
impl_wake_up_input_nm!(0, 8, WKINTH_0);
impl_wake_up_input_nm!(1, 1, WKINTA_1);
impl_wake_up_input_nm!(1, 2, WKINTB_1);
impl_wake_up_input_nm!(1, 3, WKINTC_1);
impl_wake_up_input_nm!(1, 4, WKINTD_1);
impl_wake_up_input_nm!(1, 5, WKINTE_1);
impl_wake_up_input_nm!(1, 6, WKINTF_1);
impl_wake_up_input_nm!(1, 7, WKINTG_1);
impl_wake_up_input_nm!(1, 8, WKINTH_1);
impl_wake_up_input_nm!(2, 1, WKINTA_2);
impl_wake_up_input_nm!(2, 2, WKINTB_2);
impl_wake_up_input_nm!(2, 3, WKINTC_2);
impl_wake_up_input_nm!(2, 4, WKINTD_2);
impl_wake_up_input_nm!(2, 5, WKINTE_2);
impl_wake_up_input_nm!(2, 6, WKINTF_2);
impl_wake_up_input_nm!(2, 7, WKINTG_2);
impl_wake_up_input_nm!(2, 8, WKINTH_2);
