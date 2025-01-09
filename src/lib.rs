#![no_std]

pub mod gpio;
pub mod miwu;
pub use npcx490m_pac as pac;

embassy_hal_internal::peripherals!(
    PA02, PA03, PA04, PA09, PA10, PA11, PA12, PB02, PB03, PB04, PB05, PB06, PB07, PB08, PB09, PB10, PB11, PB12, PC01,
    PC02, PC03, PC04, PC05, PC06, PC07, PC08, PC09, PC10, PC11, PC12, PD02, PD03, PD04, PD05, PD06, PD07, PD08, PD09,
    PD10, PD11, PE02, PE03, PE04, PE05, PE06, PE07, PE08, PE09, PE10, PE11, PF02, PF03, PF04, PF05, PF06, PF07, PF08,
    PF09, PF10, PF11, PF12, PG02, PG03, PG04, PG05, PG06, PG07, PG08, PG09, PG10, PG11, PG12, PH01, PH02, PH03, PH04,
    PH05, PH06, PH07, PH08, PH09, PH10, PH11, PI01, PI02, PI03, PI04, PI05, PI06, PI07, PI08, PI09, PI10, PI11, PI12,
    PJ01, PJ02, PJ03, PJ04, PJ05, PJ06, PJ07, PJ08, PJ09, PJ10, PJ11, PK01, PK02, PK03, PK04, PK05, PK06, PK07, PK08,
    PK09, PK10, PK11, PK12, PL01, PL02, PL03, PL05, PL06, PL07, PL08, PL09, PL10, PL11, PL12, PM02, PM01, PM04, PM05,
    PM06, PM07, PM11, PM12, MIWU1_73
);

pub fn init() -> Peripherals {
    Peripherals::take()
}
