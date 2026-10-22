//! Шаблоны строк лога и классификация протоколов. Перенос `core/patterns.ts`.
//!
//! Категория лога меняется от сезона к сезону (GameLog:, TLLua:, TLShipping:,
//! TLNetGame:) — матчим любую. Классы JS `\d` и `\w` — ASCII, поэтому здесь
//! они записаны явно ([0-9], [A-Za-z0-9_]): у регулярок Rust они Unicode.

use std::sync::OnceLock;

use regex::Regex;

use crate::types::{ConfigBaseId, ProtoKind};

const D: &str = "[0-9]";
const W: &str = "[A-Za-z0-9_]";

fn category() -> String {
    format!(r"{W}+:\s*Display:\s*\[Game\]\s*")
}

fn rx(src: &str) -> Regex {
    Regex::new(&src.replace(r"\d", D).replace(r"\w", W)).expect("шаблон лога")
}

pub struct Patterns {
    pub timestamp: Regex,
    pub bag_modify: Regex,
    pub bag_init: Regex,
    pub bag_remove: Regex,
    pub item_change: Regex,
    pub item_add: Regex,
    pub gear_identity: Regex,
    pub enter_level: Regex,
    pub level_path: Regex,
    pub level_ids: Regex,
    pub player_login: Regex,
    pub socket_start: Regex,
    pub socket_end: Regex,
    pub body_prefix: Regex,
    pub body_key: Regex,
    pub body_value: Regex,
    pub season_id: Regex,
    pub hub: Regex,
}

pub fn patterns() -> &'static Patterns {
    static P: OnceLock<Patterns> = OnceLock::new();
    P.get_or_init(|| {
        let c = category();
        Patterns {
            // Метка времени UE: [2026.08.10-10.04.54:717][838]
            timestamp: rx(r"^\[(\d{4})\.(\d{2})\.(\d{2})-(\d{2})\.(\d{2})\.(\d{2}):(\d{3})\]"),
            // Num — НОВЫЙ РАЗМЕР СТАКА в слоте, а не прирост.
            bag_modify: rx(&format!(
                r"{c}BagMgr@:Modfy\s+BagItem\s+PageId\s*=\s*(\d+)\s+SlotId\s*=\s*(\d+)\s+ConfigBaseId\s*=\s*(\d+)\s+Num\s*=\s*(\d+)"
            )),
            bag_init: rx(&format!(
                r"{c}BagMgr@:InitBagData\s+PageId\s*=\s*(\d+)\s+SlotId\s*=\s*(\d+)\s+ConfigBaseId\s*=\s*(\d+)\s+Num\s*=\s*(\d+)"
            )),
            bag_remove: rx(&format!(r"{c}BagMgr@:RemoveBagItem\s+PageId\s*=\s*(\d+)\s+SlotId\s*=\s*(\d+)")),
            item_change: rx(&format!(r"{c}ItemChange@\s*ProtoName=(\w+)\s+(start|end)")),
            // ItemChange@ Add Id=10321_25b2b109-… BagNum=1 in PageId=103 SlotId=53
            item_add: rx(&format!(r"{c}ItemChange@\s*Add\s+Id=(\d+)_[0-9a-fA-F-]+\s+BagNum=(\d+)\s+in\s+PageId=(\d+)\s+SlotId=(\d+)")),
            // Сравнение при наведении на вещь: база и легендарка на ней (0 — не легендарка).
            // AttributeMgr:SetPlayerEquipment(BaseId=4647, GoldId=113107, 6)
            gear_identity: rx(&format!(r"{c}AttributeMgr:SetPlayerEquipment\(BaseId=(\d+),\s*GoldId=(\d+)")),
            enter_level: rx(&format!(r"{c}LevelMgr:_EnterLevelSuccess\(\)\s*(\d+)")),
            // Ассет — папка, в которой лежит .umap.
            level_path: rx(&format!(r"{c}LevelMgr@:LevelPath,\s*Model\s*=\s*Content/Art/(?:[^\s/]+/)*([^\s/]+)/[^\s/]+\.umap")),
            level_ids: rx(&format!(r"{c}LevelMgr@\s+LevelUid,\s*LevelType,\s*LevelId\s*=\s*(\d+)\s+(\d+)\s+(\d+)")),
            player_login: rx(&format!(r"{c}PlayerIdTrace@SelectRoleNetMgr:_UserLoginStep3\s+buildLoginParm\s+playerId=(\d+)")),
            socket_start: rx(&format!(
                r"{c}-+Socket\s+(SendMessage|RecvMessage|PushMessage|RecvPushMsg)\s+STT-+([^-]+)-+SynId\s*=\s*(\d+)"
            )),
            socket_end: rx(&format!(r"{c}-+Socket\s+(?:SendMessage|RecvMessage|PushMessage|RecvPushMsg)\s+End-+")),
            body_prefix: rx(r"^\[[^\]]*\]\[[^\]]*\][^:\[\]]*:\s*Display:\s*\[Game\]\s?"),
            body_key: rx(r"\+([^+\s\[\]|]+)"),
            body_value: rx(r"\[([^\]]*)\]\s*$"),
            season_id: rx(r"\+SeasonId \[(\d+)\]"),
            hub: Regex::new(r"(?i)hideout|town|hub|lobby|social|LoginScene|SelectRole|YuJinZhiXiBiNanSuo|ZhuCheng").unwrap(),
        }
    })
}

/// Классификация ProtoName. От неё зависит, попадёт ли изменение сумки в доход.
pub fn proto_kind(proto: &str) -> ProtoKind {
    use ProtoKind::*;
    match proto {
        // доход
        "PickItem" | "PickItems" => Loot,
        // расход на открытие карты
        "Spv3Open" | "ClimbTowerOpen" => MapCost,
        // биржа
        "XchgForSale" | "XchgBuy" | "XchgReceive" | "XchgRecall" => Trade,
        // торговец-NPC, перекладывание, укрытие — не доход и не расход
        "BuyVendorGoods" | "RefreshVendorShop" | "Push2" | "PutOn" | "PutOff" | "BuySkill" | "LockOrUnlockItem"
        | "Evaluate" | "EvaluateOneKey" | "ExchangeItem" | "MoveWareHouseItems2" | "MoveWareHouseItems"
        | "ResetItemsLayout" | "WashGeniusAllPoint" | "ChooseForgeItem" | "ReCostItem" | "TalentBlockEquip"
        | "TalentBlockUnequip" | "TalentBlockEnhance" | "TalentBlockReshape" | "TalentBlockRotate" | "UnequipSkill"
        | "OpenRewardBox" | "Spv3RefreshAffix" => Neutral,
        _ => Unknown,
    }
}

/// Типы предметов-ключей от карт по type_cn справочника: компас, сезонный компас, маяк.
pub fn is_map_key_type(type_cn: Option<&str>) -> bool {
    matches!(type_cn, Some("罗盘") | Some("赛季罗盘") | Some("信标"))
}

/// Является ли предмет ключом от карты — знает справочник.
pub type MapKeyLookup<'a> = &'a dyn Fn(ConfigBaseId) -> bool;

/// Смысл конкретного изменения сумки: Push2 с минусом по ключу от карты — вложение (SS14).
pub fn delta_kind(proto: &str, id: ConfigBaseId, qty: i64, is_map_key: Option<MapKeyLookup>) -> ProtoKind {
    if proto == "Push2" && qty < 0 && is_map_key.is_some_and(|f| f(id)) {
        return ProtoKind::MapCost;
    }
    proto_kind(proto)
}

/// Города и укрытия: забеги в них не считаются фармом.
pub fn is_hub_asset(asset: Option<&str>) -> bool {
    asset.is_some_and(|a| patterns().hub.is_match(a))
}

/// Зоны-испытания (сезонная тюрьма, LevelType 28): каждый вход — новый забег.
pub fn is_challenge_level(level_type: Option<i64>) -> bool {
    level_type == Some(28)
}
