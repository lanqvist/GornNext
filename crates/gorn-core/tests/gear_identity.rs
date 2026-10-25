//! Опознание легендарки: подбор пишет базу («Кольцо духа урожая», 4647), а
//! пробивка и сравнение — саму легендарку («Horn Ring», 113107). Выпавший
//! экземпляр получает её название и цену, а не цену редких роллов базы;
//! переживает перезапуск.

use std::sync::Arc;

use gorn_core::engine::{CatalogueItem, Engine, EngineOptions, NameLanguage};
use gorn_core::{analyze_bytes, AnalyzeOptions};

fn line(t: &str, body: &str) -> String {
    format!("[2026.10.20-{t}:000][  1]TLLua: Display: [Game] {body}")
}

fn catalogue() -> Vec<CatalogueItem> {
    let gear = |id, name: &str, base| CatalogueItem {
        id,
        name: name.into(),
        name_ru: None,
        type_cn: None,
        icon: None,
        gear: true,
        base,
        legendary: base.is_some(),
    };
    vec![gear(4647, "Harvest Spirit Ring", None), gear(113107, "Horn Ring", Some(4647))]
}

fn engine(data: &std::path::Path, logs: &std::path::Path) -> Engine {
    let mut e = Engine::new(
        data,
        &catalogue(),
        EngineOptions { instance_prices: true, community_prices: false, language: NameLanguage::En, ..Default::default() },
        Arc::new(|| 1_792_500_000_000),
    )
    .unwrap();
    e.start(Some(logs), None).map_err(|(_, m)| m).unwrap();
    e
}

/// Карта с подбором кольца на базе 4647 и выход в укрытие.
fn map_with_ring() -> Vec<String> {
    vec![
        line("10.00.00", "BagMgr@:InitBagData PageId = 100 SlotId = 0 ConfigBaseId = 100300 Num = 5"),
        line("10.00.00", "BagMgr@:RemoveBagItem PageId = 100 SlotId = 3"),
        line("10.00.01", "LevelMgr@ LevelUid, LevelType, LevelId = 1 3 5348"),
        line("10.00.01", "LevelMgr@:LevelPath, Model = Content/Art/Maps/01SD/SD_YuanGuTongDao201/SD_YuanGuTongDao201.umap World'/Game/SD'"),
        line("10.00.02", "LevelMgr:_EnterLevelSuccess() 5348"),
        line("10.00.10", "ItemChange@ ProtoName=PickItem start"),
        line("10.00.10", "ItemChange@ Add Id=4647_8851bf88-cdf6-11f1-bfac-000000000020 BagNum=1 in PageId=100 SlotId=3"),
        line("10.00.10", "BagMgr@:Modfy BagItem PageId = 100 SlotId = 3 ConfigBaseId = 4647 Num = 1"),
        line("10.00.10", "ItemChange@ ProtoName=PickItem end"),
        line("10.01.00", "LevelMgr@ LevelUid, LevelType, LevelId = 2 0 110"),
        line("10.01.01", "LevelMgr:_EnterLevelSuccess() 110"),
    ]
}

/// Пробивка легендарки без фильтров: десять лотов по 3 ФЕ.
fn legendary_check(t: &str) -> Vec<String> {
    let mut v = vec![
        line(t, "----Socket SendMessage STT----XchgSearchPrice----SynId = 81"),
        line(t, ""),
        "+typ3 [39]".into(),
        "+filters+1+key [16]".into(),
        "|       | +refer [113107]".into(),
        "|       +2+key [21]".into(),
        line(t, "----Socket SendMessage End----"),
        line(t, "----Socket RecvMessage STT----XchgSearchPrice----SynId = 81"),
        "+prices+1+unitPrices+1 [3.0]".into(),
    ];
    for i in 2..=10 {
        v.push(format!("|      | |          +{i} [3.0]"));
    }
    v.push(line(t, "----Socket RecvMessage End----"));
    v
}

fn ring_of(snapshot: &serde_json::Value) -> serde_json::Value {
    let runs = snapshot["recent"].as_array().unwrap();
    let drops: Vec<&serde_json::Value> = runs.iter().flat_map(|r| r["drops"].as_array().unwrap()).collect();
    assert!(drops.iter().all(|d| d["configBaseId"] != 4647), "база осталась базой: {drops:?}");
    drops.into_iter().find(|d| d["configBaseId"] == 113107).cloned().expect("нет опознанного кольца")
}

#[test]
fn legendary_check_identifies_the_dropped_base() {
    let root = std::env::temp_dir().join(format!("gorn-gear-identity-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let (data, logs) = (root.join("data"), root.join("logs"));
    std::fs::create_dir_all(&data).unwrap();
    std::fs::create_dir_all(&logs).unwrap();

    let mut e = engine(&data, &logs);
    let mut lines = map_with_ring();
    lines.extend(legendary_check("10.02.00"));
    // Сравнение при наведении — та же легендарка; повтор ничего не меняет.
    lines.push(line("10.02.05", "AttributeMgr:SetPlayerEquipment(BaseId=4647, GoldId=113107, 6)"));
    lines.push(line("10.02.05", "AttributeMgr:SetPlayerEquipment(BaseId=4647, GoldId=113107, 7)"));
    e.consume(&lines);
    let ring = ring_of(&serde_json::to_value(e.build_snapshot()).unwrap());
    assert_eq!(ring["name"], "Horn Ring", "{ring}");
    assert_eq!(ring["unitFe"].as_f64(), Some(3.0), "{ring}");
    assert_eq!(ring["priceSource"], "own");

    // После перезапуска (лога нет — только база) кольцо остаётся легендаркой.
    e.stop();
    drop(e);
    let e = engine(&data, &logs);
    let ring = ring_of(&serde_json::to_value(e.build_snapshot()).unwrap());
    assert_eq!(ring["name"], "Horn Ring", "{ring}");
    let _ = std::fs::remove_dir_all(&root);
}

/// Неопознанная база (редкая шмотка) и «Флешбэк героя» вне учёта добычи: цена
/// сообщества у них одна на все роллы. Ни строк добычи, ни ФЕ — даже после
/// пробивки базы с фильтрами.
#[test]
fn rare_gear_and_hero_memory_are_not_loot() {
    let root = std::env::temp_dir().join(format!("gorn-gear-untracked-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let (data, logs) = (root.join("data"), root.join("logs"));
    std::fs::create_dir_all(&data).unwrap();
    std::fs::create_dir_all(&logs).unwrap();

    let mut items = catalogue();
    items.push(CatalogueItem { id: 71001, name: "Memory of Origin".into(), type_cn: Some("英雄追忆".into()), ..Default::default() });
    let mut e = Engine::new(
        &data,
        &items,
        EngineOptions { instance_prices: true, community_prices: false, language: NameLanguage::En, ..Default::default() },
        Arc::new(|| 1_792_500_000_000),
    )
    .unwrap();
    e.start(Some(&logs), None).map_err(|(_, m)| m).unwrap();
    let mut lines = map_with_ring();
    lines.insert(9, line("10.00.11", "ItemChange@ ProtoName=PickItem start"));
    lines.insert(10, line("10.00.11", "ItemChange@ Add Id=71001_8851bf88-cdf6-11f1-bfac-000000000021 BagNum=1 in PageId=100 SlotId=4"));
    lines.insert(11, line("10.00.11", "BagMgr@:Modfy BagItem PageId = 100 SlotId = 4 ConfigBaseId = 71001 Num = 1"));
    lines.insert(12, line("10.00.11", "ItemChange@ ProtoName=PickItem end"));
    lines.extend(legendary_check("10.02.00").into_iter().map(|l| l.replace("[113107]", "[4647]")));
    e.consume(&lines);
    let snap = serde_json::to_value(e.build_snapshot()).unwrap();
    let run = &snap["recent"][0];
    assert_eq!(run["drops"], serde_json::json!([]), "{run}");
    assert_eq!(run["valueFe"].as_f64(), Some(0.0), "{run}");
    let _ = std::fs::remove_dir_all(&root);
}

/// Настоящий справочник снаряжения знает базу каждой легендарки — без неё
/// пробивка цены легендарки не опознаёт выпавшую базу.
#[test]
fn gear_catalogue_knows_legendary_bases() {
    let gear = CatalogueItem::parse_list(include_str!("../data/gear.json")).unwrap();
    let ring = gear.iter().find(|g| g.id == 113107).expect("Horn Ring");
    assert!(ring.legendary && ring.base == Some(4647));
    assert!(gear.iter().filter(|g| g.legendary).all(|g| g.base.is_some()));
    assert!(gear.iter().filter(|g| !g.legendary).all(|g| g.base.is_none()));
}

/// «Уголок божественности» и «Родословная богов» вне учёта, как и их базы:
/// пробивка 1005 не делает выпавшую базу 261005 добычей.
#[test]
fn corner_and_pedigree_are_not_loot() {
    let root = std::env::temp_dir().join(format!("gorn-gear-corner-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let (data, logs) = (root.join("data"), root.join("logs"));
    std::fs::create_dir_all(&data).unwrap();
    std::fs::create_dir_all(&logs).unwrap();

    let items = vec![
        CatalogueItem { id: 1005, name: "A Corner of Divinity".into(), legendary: true, base: Some(261005), ..Default::default() },
        CatalogueItem { id: 4647, name: "Harvest Spirit Ring".into(), gear: true, ..Default::default() },
    ];
    let mut e = Engine::new(
        &data,
        &items,
        EngineOptions { instance_prices: true, community_prices: false, language: NameLanguage::En, ..Default::default() },
        Arc::new(|| 1_792_500_000_000),
    )
    .unwrap();
    e.start(Some(&logs), None).map_err(|(_, m)| m).unwrap();
    let lines: Vec<String> = map_with_ring()
        .into_iter()
        .chain(legendary_check("10.02.00"))
        .map(|l| l.replace("4647", "261005").replace("[113107]", "[1005]"))
        .collect();
    e.consume(&lines);
    let snap = serde_json::to_value(e.build_snapshot()).unwrap();
    let run = &snap["recent"][0];
    assert_eq!(run["drops"], serde_json::json!([]), "{run}");
    assert_eq!(run["valueFe"].as_f64(), Some(0.0), "{run}");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn hover_identifies_and_log_scan_repeats_it() {
    let mut lines = map_with_ring();
    lines.push(line("10.02.05", "AttributeMgr:SetPlayerEquipment(BaseId=4647, GoldId=113107, 6)"));
    let text = lines.join("\n") + "\n";
    let bases = |id: i64| (id == 113107).then_some(4647);
    let ids = |gear_base| -> Vec<i64> {
        let r = analyze_bytes(text.as_bytes(), AnalyzeOptions { is_map_key: None, current: false, gear_base });
        r.runs.iter().flat_map(|run| run.drops.iter().map(|d| d.config_base_id)).collect()
    };
    assert_eq!(ids(Some(&bases)), vec![113107]);
    // Сверка с эталоном — без опознания.
    assert_eq!(ids(None), vec![4647]);
    // Не легендарка (GoldId=0) — база остаётся базой.
    let rare = text.replace("GoldId=113107", "GoldId=0");
    let r = analyze_bytes(rare.as_bytes(), AnalyzeOptions { is_map_key: None, current: false, gear_base: Some(&bases) });
    assert_eq!(r.runs.iter().flat_map(|run| run.drops.iter().map(|d| d.config_base_id)).collect::<Vec<_>>(), vec![4647]);
}

#[test]
fn check_after_restart_identifies_a_stored_run() {
    let root = std::env::temp_dir().join(format!("gorn-gear-stored-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let (data, logs) = (root.join("data"), root.join("logs"));
    std::fs::create_dir_all(&data).unwrap();
    std::fs::create_dir_all(&logs).unwrap();

    let mut e = engine(&data, &logs);
    e.consume(&map_with_ring());
    e.stop();
    drop(e);
    // Забег уже в истории из базы — пробивка после перезапуска переписывает и её.
    let mut e = engine(&data, &logs);
    e.consume(&legendary_check("10.03.00"));
    assert_eq!(ring_of(&serde_json::to_value(e.build_snapshot()).unwrap())["unitFe"].as_f64(), Some(3.0));
    e.stop();
    drop(e);
    let e = engine(&data, &logs);
    assert_eq!(ring_of(&serde_json::to_value(e.build_snapshot()).unwrap())["name"], "Horn Ring");
    let _ = std::fs::remove_dir_all(&root);
}
