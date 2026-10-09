//! MSI Afterburner (MAHM) 共享内存读取

use crate::shared_mem::*;
use crate::util::Mapping;

const MAHM_NAME: &str = "MAHMSharedMemory";

const OFF_SIG: usize = 0;
const OFF_HDR_SIZE: usize = 8;
const OFF_NUM_ENTRIES: usize = 12;
const OFF_ENTRY_SIZE: usize = 16;

/// 单个条目内偏移
const M: usize = 260;
const E_SRCNAME: usize = 0;
const E_UNITS: usize = M;
const E_DATA: usize = M * 5;
const E_FLAGS: usize = E_DATA + 12;
const E_GPU: usize = E_FLAGS + 4;
const E_SRCID: usize = E_GPU + 4;

pub fn read() -> MahmData {
    let map = match Mapping::open(MAHM_NAME) {
        Some(m) => m,
        None => {
            return MahmData {
                ok: false,
                all_count: 0,
                osd_count: 0,
                items: Vec::new(),
                error: Some("Afterburner 未运行（未找到 MAHM 共享内存）".into()),
            }
        }
    };

    // 校验签名
    if map.u32_at(OFF_SIG) != MAHM_SIG {
        return MahmData {
            ok: false,
            all_count: 0,
            osd_count: 0,
            items: Vec::new(),
            error: Some("MAHM 签名不匹配".into()),
        };
    }

    let hdr_size = map.u32_at(OFF_HDR_SIZE) as usize;
    let num = map.u32_at(OFF_NUM_ENTRIES);
    let esize = map.u32_at(OFF_ENTRY_SIZE) as usize;

    if num == 0 || esize == 0 {
        return MahmData {
            ok: true,
            all_count: 0,
            osd_count: 0,
            items: Vec::new(),
            error: None,
        };
    }

    let mut items = Vec::new();
    let mut osd_count = 0u32;
    let mut seen: std::collections::HashSet<(u32, u32)> = std::collections::HashSet::new();

    for i in 0..num as usize {
        let base = hdr_size + i * esize;
        let flags = map.u32_at(base + E_FLAGS);

        // 只读取 Afterburner 里勾选了「在 OSD 上显示」的项
        if (flags & MAHM_FLAG_SHOW_IN_OSD) == 0 {
            continue;
        }
        osd_count += 1;

        let src_id = map.u32_at(base + E_SRCID);
        let gpu = map.u32_at(base + E_GPU);

        // 去重：同 (源ID, GPU) 只保留第一个
        if !seen.insert((src_id, gpu)) {
            continue;
        }

        let value = map.f32_at(base + E_DATA);
        // Afterburner 用 FLT_MAX 表示数据当前不可用（如没游戏时的帧率）
        if value.is_nan() || value >= FLT_MAX * 0.99 {
            continue;
        }

        let srcname = map.cstr_at(base + E_SRCNAME, M);
        let units = map.cstr_at(base + E_UNITS, M);
        let label = source_label(src_id);
        let label = if label.is_empty() {
            srcname.clone()
        } else {
            label.to_string()
        };

        items.push(SensorItem {
            id: src_id,
            name: srcname,
            label,
            source: source_label(src_id).to_string(),
            group: source_group(src_id).to_string(),
            value: (value * 100.0).round() / 100.0,
            unit: norm_unit(&units),
            gpu,
        });
    }

    MahmData {
        ok: true,
        all_count: num,
        osd_count,
        items,
        error: None,
    }
}
