//! Числа из таблиц клиента игры для выбора событий и орудий.
//!
//! Перенос `src/core/prison/client-tables.ts` (gorn-tli) сразу в виде
//! массивов по номеру события / орудия — так их держит и rules.ts.
//! Источник чисел — расшифрованные Lua-таблицы S16, см. эталон.

use crate::model::{event, tool, TOOL_COUNT};

/// Где может появиться событие: SpawnCellState 801 any, 802 unopened, 803 opened.
pub const SPAWN_ANY: u8 = 0;
pub const SPAWN_OPENED: u8 = 1;
pub const SPAWN_UNOPENED: u8 = 2;

/// Базовый Weight события по номеру (EVENT_IDS).
pub const EVENT_W: [i32; 17] = [
    20, // chain 242
    20, // execution 222
    40, // behind 203
    30, // appetite 211
    40, // convict 201
    10, // raven 231
    40, // freedom 202
    10, // usurp1 252
    0,  // usurp2 253: базовый вес 0, движок берёт Params.usurp_next_weight
    0,  // usurp3 254
    0,  // usurp4 255
    20, // ratway 241
    10, // treasure 232
    30, // rival 221
    20, // luck 213
    20, // escort 251
    30, // extra 212
];

pub const EVENT_SPAWN: [u8; 17] = [
    SPAWN_ANY,      // chain
    SPAWN_OPENED,   // execution
    SPAWN_UNOPENED, // behind
    SPAWN_OPENED,   // appetite
    SPAWN_UNOPENED, // convict
    SPAWN_ANY,      // raven
    SPAWN_UNOPENED, // freedom
    SPAWN_ANY,      // usurp1
    SPAWN_ANY,      // usurp2
    SPAWN_ANY,      // usurp3
    SPAWN_ANY,      // usurp4
    SPAWN_ANY,      // ratway
    SPAWN_ANY,      // treasure
    SPAWN_OPENED,   // rival
    SPAWN_OPENED,   // luck
    SPAWN_OPENED,   // escort
    SPAWN_OPENED,   // extra
];

/// Маска орудий, при которых событие не выпадает (ExcludeRelic).
pub const EVENT_EXCL: [i32; 17] = {
    let mut a = [0i32; 17];
    a[event::FREEDOM as usize] = 1 << tool::MUZZLE; // 202, ExcludeRelic [101]
    a
};

// Особые орудия: пара одного зверя совместима, с остальными особыми — нет (Exclude 141–148).
const ELEPHANT: i32 = (1 << tool::TONGS) | (1 << tool::MAUL);
const LION_PAIR: i32 = (1 << tool::TRAP) | (1 << tool::CROWN);
const WOLF_PAIR: i32 = (1 << tool::CLAWS) | (1 << tool::GUILLOTINE);
const RAT_PAIR: i32 = (1 << tool::IRONBOX) | (1 << tool::SKEWER);
const SPECIAL: i32 = ELEPHANT | LION_PAIR | WOLF_PAIR | RAT_PAIR;

const fn pair_of(t: u32) -> i32 {
    let bit = 1 << t;
    if ELEPHANT & bit != 0 {
        ELEPHANT
    } else if LION_PAIR & bit != 0 {
        LION_PAIR
    } else if WOLF_PAIR & bit != 0 {
        WOLF_PAIR
    } else if RAT_PAIR & bit != 0 {
        RAT_PAIR
    } else {
        0
    }
}

/// Базовый Weight орудия: 40 у обычных (101–132), 5 у особых (141–148).
pub const TOOL_W: [i32; TOOL_COUNT] = {
    let mut a = [40i32; TOOL_COUNT];
    let mut t = 0;
    while t < TOOL_COUNT {
        if SPECIAL & (1 << t) != 0 {
            a[t] = 5;
        }
        t += 1;
    }
    a
};

/// Маска орудий, исключённых этим орудием (Exclude).
pub const TOOL_EXCL: [i32; TOOL_COUNT] = {
    let mut a = [0i32; TOOL_COUNT];
    let mut t = 0;
    while t < TOOL_COUNT {
        if SPECIAL & (1 << t) != 0 {
            a[t] = SPECIAL & !pair_of(t as u32);
        }
        t += 1;
    }
    a
};

/// Вариантов в выборе орудия (config 320).
pub const TOOL_OFFER: i32 = 3;
