//! Live vendor catalog refresh: availability, models, and usage for every
//! subscription runtime installed on this machine. Read-only; no turns.
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = shadowcode_core::cli_agent::CliAgentsConfig::default();
    let catalog = shadowcode_core::cli_agent::catalog::VendorCatalog::new();
    let started = std::time::Instant::now();
    for status in catalog.refresh_all(&config, true).await {
        println!(
            "== {:?}: {} | {} | images={} asks_approval={} models={} error={:?}",
            status.vendor,
            status.availability.label(),
            status.detail,
            status.accepts_images,
            status.asks_approval,
            status.models.len(),
            status.error
        );
        for m in status.models.iter().take(4) {
            println!(
                "   model {} ({}) default={} vision={} | usage: {}",
                m.id,
                m.label,
                m.is_default,
                m.vision,
                status.usage_for(&m.id, shadowcode_core::now()).label
            );
        }
        if status.models.len() > 4 {
            println!("   … {} more", status.models.len() - 4);
        }
    }
    println!("refresh took {:.1}s", started.elapsed().as_secs_f64());
    let rows = catalog.picker_rows(&config, false).await;
    println!("picker rows: {}", rows.len());
    for row in rows.iter().filter(|r| {
        r.is_default || r.availability != shadowcode_core::cli_agent::picker::Availability::Ready
    }) {
        println!(
            "   {} | {} | {} | vision={} | {}",
            row.id, row.name, row.availability_label, row.vision, row.usage.label
        );
    }
    Ok(())
}
