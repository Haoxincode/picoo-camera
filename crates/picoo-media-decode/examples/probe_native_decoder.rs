fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut decoder = picoo_media_decode::create_platform_decoder();
    let capabilities = picoo_media_decode::probe_capabilities(decoder.as_mut())?;
    println!("Native decoder offers: {}", capabilities.offers.len());
    for offer in capabilities.offers {
        println!("{offer:?}");
    }
    Ok(())
}
