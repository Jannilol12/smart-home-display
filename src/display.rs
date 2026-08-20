use epd_waveshare::{epd7in5_v2::Epd7in5, prelude::WaveshareDisplay};
use esp_idf_svc::hal::{
    delay::FreeRtos, // 1. Switched from Ets to FreeRtos to stop CPU starvation
    gpio::{AnyIOPin, Input, Output, PinDriver, Pull},
    spi::{
        config::{Config as SpiConfig, DriverConfig},
        Dma, SpiDeviceDriver, SpiDriver, SPI2,
    },
    units::FromValueType,
};

pub type MyEpd<'a> = Epd7in5<
    SpiDeviceDriver<'a, SpiDriver<'a>>,
    PinDriver<'a, Input>,
    PinDriver<'a, Output>,
    PinDriver<'a, Output>,
    FreeRtos, // Updated here
>;

pub struct DisplayHardware<'a> {
    pub epd: MyEpd<'a>,
    pub spi_device: SpiDeviceDriver<'a, SpiDriver<'a>>,
    pub delay: FreeRtos, // Updated here
}

pub fn setup_hardware<'a>(
    spi2: SPI2<'a>,
    sclk: AnyIOPin<'a>,
    mosi: AnyIOPin<'a>,
    cs_pin: AnyIOPin<'a>,
    busy_pin: AnyIOPin<'a>,
    rst_pin: AnyIOPin<'a>,
    dc_pin: AnyIOPin<'a>,
) -> anyhow::Result<DisplayHardware<'a>> {
    // 2. Turn on Direct Memory Access (DMA) so the 48KB image doesn't crash the SPI bus
    let driver_config = DriverConfig::new().dma(Dma::Auto(4096));

    let spi_driver = SpiDriver::new(
        spi2,
        sclk,
        mosi,
        Option::<AnyIOPin<'a>>::None,
        &driver_config,
    )?;

    let spi_config = SpiConfig::new().baudrate(8_u32.MHz().into());
    let mut spi_device = SpiDeviceDriver::new(spi_driver, Some(cs_pin), &spi_config)?;

    let busy_in = PinDriver::input(busy_pin, Pull::Floating)?;
    let rst = PinDriver::output(rst_pin)?;
    let dc = PinDriver::output(dc_pin)?;

    let mut delay = FreeRtos; // Updated here

    let epd = Epd7in5::new(&mut spi_device, busy_in, dc, rst, &mut delay, None)
        .map_err(|_| anyhow::anyhow!("Failed to initialize EPD"))?;

    Ok(DisplayHardware {
        epd,
        spi_device,
        delay,
    })
}
