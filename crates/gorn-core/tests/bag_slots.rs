//! Сумка по слотам: «Упорядочить» и вход другим персонажем переписывают
//! страницу целиком (ItemChange@ Reset) — старые слоты не остаются в сумке.

use std::sync::Arc;

use gorn_core::engine::{Engine, EngineOptions};

fn line(t: &str, body: &str) -> String {
    format!("[2026.10.24-{t}:000][  1]TLLua: Display: [Game] {body}")
}

fn init(t: &str, page: i64, slot: i64, id: i64, num: i64) -> String {
    line(t, &format!("BagMgr@:InitBagData PageId = {page} SlotId = {slot} ConfigBaseId = {id} Num = {num}"))
}

/// (страница, слот, предмет, количество) всех занятых слотов.
fn slots(e: &Engine) -> Vec<(i64, i64, i64, i64)> {
    e.query_bag()
        .pages
        .iter()
        .flat_map(|p| p.slots.iter().map(move |s| (p.page_id, s.slot_id, s.item.config_base_id, s.item.qty)))
        .collect()
}

#[test]
fn reset_rewrites_the_page() {
    let root = std::env::temp_dir().join(format!("gorn-bag-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let (data, logs) = (root.join("data"), root.join("logs"));
    std::fs::create_dir_all(&data).unwrap();
    std::fs::create_dir_all(&logs).unwrap();
    let mut e = Engine::new(
        &data,
        &[],
        EngineOptions { community_prices: false, ..Default::default() },
        Arc::new(|| 1_792_500_000_000),
    )
    .unwrap();
    e.start(Some(&logs), None).map_err(|(_, m)| m).unwrap();

    // Вход: компасы в слотах 4 и 14, ФЕ на странице валюты.
    let mut lines = vec![
        line("07.45.00", "ItemChange@ Reset PageId=102"),
        line("07.45.00", "ItemChange@ Reset PageId=103"),
        init("07.45.00", 102, 1, 100300, 857),
        init("07.45.00", 103, 4, 10330, 24),
        init("07.45.00", 103, 14, 10320, 2),
    ];
    e.consume(&lines);
    assert_eq!(slots(&e), vec![(102, 1, 100300, 857), (103, 4, 10330, 24), (103, 14, 10320, 2)]);

    // «Упорядочить» страницу 103: всё съехало в начало.
    lines = vec![
        line("07.45.50", "ItemChange@ ProtoName=ResetItemsLayout start"),
        line("07.45.50", "ItemChange@ Reset PageId=103"),
        line("07.45.50", "ItemChange@ Add Id=10330_bde1efe4-c948-11f1-b475-00000000000b BagNum=24 in PageId=103 SlotId=0"),
        line("07.45.50", "ItemChange@ Add Id=10320_bde1efe4-c948-11f1-b475-00000000000c BagNum=2 in PageId=103 SlotId=1"),
        line("07.45.50", "ItemChange@ ProtoName=ResetItemsLayout end"),
        init("07.45.50", 102, 1, 100300, 857),
        init("07.45.50", 103, 0, 10330, 24),
        init("07.45.50", 103, 1, 10320, 2),
    ];
    e.consume(&lines);
    assert_eq!(slots(&e), vec![(102, 1, 100300, 857), (103, 0, 10330, 24), (103, 1, 10320, 2)]);
    let bag = e.build_snapshot().bag.unwrap();
    assert_eq!(bag.fe_cash, 857);

    // Другой персонаж: прочее у него пусто, но Reset об этом говорит.
    lines = vec![
        line("08.01.05", "ItemChange@ Reset PageId=102"),
        line("08.01.05", "ItemChange@ Reset PageId=103"),
        init("08.01.05", 102, 0, 100300, 12),
    ];
    e.consume(&lines);
    assert_eq!(slots(&e), vec![(102, 0, 100300, 12)]);
    let _ = std::fs::remove_dir_all(&root);
}
