use crate::models::{Artifact, QualityMetric, QualityReport, QualityWarning};

pub fn analyze_artifact(artifact: &Artifact) -> QualityReport {
    let text = artifact.content.trim();
    let char_count = text.chars().filter(|c| !c.is_whitespace()).count();
    let paragraphs = paragraphs(text);
    let paragraph_count = paragraphs.len();
    let dialogue_line_count = paragraphs
        .iter()
        .filter(|line| is_dialogue_line(line))
        .count();
    let dialogue_density = ratio(dialogue_line_count, paragraph_count);
    let avg_paragraph_chars = ratio(char_count, paragraph_count).round() as usize;
    let long_paragraph_count = paragraphs
        .iter()
        .filter(|line| line.chars().filter(|c| !c.is_whitespace()).count() >= 220)
        .count();
    let repeated_simile_count = count_template_similes(text);
    let explanation_marker_count = count_any(
        text,
        &[
            "这说明",
            "也就是说",
            "事实上",
            "他意识到",
            "她意识到",
            "很显然",
            "换句话说",
            "原因是",
        ],
    );
    let reveal_overload_count = count_reveal_overload_signals(text);
    let reveal_density = if char_count == 0 {
        0.0
    } else {
        reveal_overload_count as f64 / (char_count as f64 / 1000.0).max(1.0)
    };
    let opening_momentum = has_opening_momentum(text);
    let ending_progression = has_ending_progression(text);
    let sensory_detail_score = sensory_detail_score(text);
    let professional_detail_score = professional_detail_score(text);
    let has_markdown_title = text
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(|line| line.starts_with('#'))
        .unwrap_or(false);

    let is_body = artifact.stage == "draft" || artifact.stage == "revision";
    let mut score: i64 = if is_body { 72 } else { 76 };
    let mut warnings = Vec::new();

    if is_body {
        let narrative_loses_focus = (long_paragraph_count >= 3 || repeated_simile_count >= 3)
            && explanation_marker_count >= 3
            || (reveal_overload_count >= 7 && reveal_density >= 2.8);
        if narrative_loses_focus {
            score -= 8;
            warnings.push(warning(
                "叙事失焦",
                "重复解释、重段落或信息堆叠同时出现，章节功能可能被稀释。",
                "保留必要的场景、对白和情绪承接；只删同功能的重复说明、重复确认和无后果过渡。",
            ));
        }

        if has_markdown_title {
            score -= 5;
            warnings.push(warning(
                "正文含标题",
                "写作/修订 Agent 应只输出章节正文，不应包含 Markdown 标题或创作说明。",
                "删除标题行，只保留正文内容。",
            ));
        }

        if !opening_momentum {
            score -= 16;
            warnings.push(warning(
                "开篇驱动力不足",
                "开篇没有明显的目标、任务、瓶颈、异常、准备动作或时间压力信号。",
                "让读者在第一屏知道本章正在做什么、为什么现在要做；发育章可以是修炼、炼药、疗伤或布局，不必硬打架。",
            ));
        } else {
            score += 5;
        }

        if !ending_progression {
            score -= 6;
            warnings.push(warning(
                "结尾功能不清",
                "结尾没有清楚落下本章结果、决定、信息变化、关系变化、行动启动或仍在生效的压力。",
                "按本章模式落稳已经发生的变化；可以安静收束，不需要另加危险或反转。",
            ));
        } else {
            score += 3;
        }

        if dialogue_density < 0.12 && paragraph_count >= 8 {
            score -= 7;
            warnings.push(warning(
                "对白密度偏低",
                "正文主要依赖叙述推进，人物关系和潜台词可能不够鲜活。",
                "加入能改变局面的对白，让人物通过说话暴露目的、隐瞒或冲突。",
            ));
        }

        if long_paragraph_count >= 3 || avg_paragraph_chars >= 180 {
            score -= 7;
            warnings.push(warning(
                "段落过重",
                "长段落较多，阅读节奏和悬念释放会被压住。",
                "把动作、反应、对白拆开，让每段只承担一个推进功能。",
            ));
        }

        if repeated_simile_count >= 2 {
            score -= 8;
            warnings.push(warning(
                "模板化比喻",
                "出现多处“像……一样”式表达，容易让文本显得通用。",
                "保留最有效的一处，其余改成动作、感官或职业细节。",
            ));
        }

        if explanation_marker_count >= 3 {
            score -= 8;
            warnings.push(warning(
                "解释感偏重",
                "解释性连接词和总结句较多，可能替代了场景呈现。",
                "让读者从行为、物件、对白和后果里理解信息。",
            ));
        }

        if reveal_overload_count >= 7 && reveal_density >= 2.8 {
            score -= 12;
            warnings.push(warning(
                "信息反转偏密",
                "本章单位字数内连续释放较多真相、规则、身份或反转信号，信息密度偏高，读者可能来不及消化当前章节功能。",
                "长章可以承载更多信息，但仍应保留最能改变主角选择的一条核心反转，其余改成线索、疑点或后续章再确认。",
            ));
        } else if reveal_overload_count >= 5 && reveal_density >= 2.0 {
            score -= 3;
            warnings.push(warning(
                "信息释放略密",
                "本章单位字数内的信息增量偏高，若同时还有新人物、新规则或新地点，可能削弱单章爽点。",
                "检查这些信息是否都服务本章主动作，删掉只负责铺大设定的说明。",
            ));
        }

        if sensory_detail_score < 38 {
            score -= 6;
            warnings.push(warning(
                "感官落点不足",
                "可感知的声音、气味、触感、光线或环境细节偏少。",
                "每个关键场景补一到两个能服务气氛和线索的具体细节。",
            ));
        } else {
            score += 3;
        }

        if professional_detail_score < 28 {
            score -= 4;
            warnings.push(warning(
                "职业/场景细节不足",
                "文本里支撑题材可信度的专业名词、流程或物件较少。",
                "补入不需要解释的流程动作或工具细节，让场景更像真实工作现场。",
            ));
        } else {
            score += 3;
        }
    }

    if artifact.stage == "setting" {
        let abstract_cost_count = count_unconstrained_mentions(
            text,
            &["气运", "因果", "天命", "业力", "命数", "存在本身"],
        );
        if abstract_cost_count >= 2 {
            score -= 12;
            warnings.push(warning(
                "代价过于抽象",
                "设定反复使用气运、因果、天命等抽象代价，后续正文难以在现场验证，也容易变成作者随时解释的万能规则。",
                "把每个抽象代价改写为可观察后果、触发时点和补救失败条件，例如伤势、资源损耗、暴露痕迹、关系破裂或明确倒计时。",
            ));
        }

        let transfer_cost_count = count_unconstrained_mentions(
            text,
            &[
                "替命",
                "代价转嫁",
                "强迫支付",
                "牺牲同伴",
                "用他人代价",
                "他人支付",
            ],
        );
        if transfer_cost_count >= 2 {
            score -= 8;
            warnings.push(warning(
                "成长代价过度外包",
                "核心成长机制多次依赖替命、转嫁或牺牲同伴，主角容易失去主动承担成本的成长线，也会把后续选择压缩成工具化取舍。",
                "保留一次高压道德困境即可；把常规解法改成主角承担伤势、资源、时间、暴露或失去机会的代价。",
            ));
        }
    }

    if !is_body && char_count < 400 {
        score -= 10;
        warnings.push(warning(
            "资料偏薄",
            "当前产物信息量较少，后续 Agent 可执行约束可能不足。",
            "补充更具体的边界、冲突来源、角色目标或章节钩子。",
        ));
    }

    if warnings.is_empty() {
        warnings.push(warning(
            "未发现明显硬伤",
            "本地规则没有捕捉到高风险信号，但仍建议用试读 Agent 和人工判断确认可用性。",
            "继续关注人物一致性、信息释放和读者追读欲。",
        ));
    }

    let score = score.clamp(0, 100) as u8;
    QualityReport {
        artifact_id: artifact.id,
        stage: artifact.stage.clone(),
        verdict: verdict(score, warnings.len()),
        score,
        summary: summary(score, is_body, &warnings),
        metrics: vec![
            metric("字数", char_count as f64, "chars", None),
            metric("段落", paragraph_count as f64, "count", None),
            metric("对白段占比", dialogue_density, "ratio", Some(0.18)),
            metric("平均段长", avg_paragraph_chars as f64, "chars", Some(130.0)),
            metric("长段落数", long_paragraph_count as f64, "count", Some(2.0)),
            metric("模板比喻", repeated_simile_count as f64, "count", Some(1.0)),
            metric(
                "解释标记",
                explanation_marker_count as f64,
                "count",
                Some(2.0),
            ),
            metric(
                "反转/真相信号",
                reveal_overload_count as f64,
                "count",
                Some(4.0),
            ),
            metric("反转/真相密度", reveal_density, "ratio", Some(1.6)),
            metric(
                "开篇驱动力",
                if opening_momentum { 1.0 } else { 0.0 },
                "bool",
                Some(1.0),
            ),
            metric(
                "结尾功能",
                if ending_progression { 1.0 } else { 0.0 },
                "bool",
                Some(1.0),
            ),
            metric("感官细节", sensory_detail_score as f64, "score", Some(55.0)),
            metric(
                "职业细节",
                professional_detail_score as f64,
                "score",
                Some(40.0),
            ),
        ],
        warnings,
    }
}

fn paragraphs(text: &str) -> Vec<&str> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect()
}

fn is_dialogue_line(line: &str) -> bool {
    line.contains('“')
        || line.contains('”')
        || line.contains('"')
        || line.starts_with('「')
        || line.starts_with('『')
}

fn ratio(numerator: usize, denominator: usize) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        numerator as f64 / denominator as f64
    }
}

fn count_any(text: &str, needles: &[&str]) -> usize {
    needles
        .iter()
        .map(|needle| text.matches(needle).count())
        .sum()
}

fn count_unconstrained_mentions(text: &str, needles: &[&str]) -> usize {
    const NEGATION_MARKERS: &[&str] = &[
        "不能",
        "不得",
        "不看",
        "看不到",
        "绝不",
        "禁止",
        "不可",
        "不允许",
        "只限",
    ];

    text.lines()
        .map(str::trim)
        .filter(|line| !NEGATION_MARKERS.iter().any(|marker| line.contains(marker)))
        .map(|line| count_any(line, needles))
        .sum()
}

fn count_template_similes(text: &str) -> usize {
    let chars: Vec<char> = text.chars().collect();
    let mut count = 0;
    for (index, ch) in chars.iter().enumerate() {
        if *ch != '像' {
            continue;
        }
        let end = (index + 18).min(chars.len());
        if chars[index..end].contains(&'一') && chars[index..end].contains(&'样') {
            count += 1;
        }
    }
    count
}

fn count_reveal_overload_signals(text: &str) -> usize {
    split_sentences(text)
        .into_iter()
        .map(reveal_signal_score)
        .sum()
}

fn split_sentences(text: &str) -> Vec<&str> {
    text.split(['\n', '。', '！', '？', '!', '?'])
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
        .collect()
}

fn reveal_signal_score(sentence: &str) -> usize {
    let explicit = count_any(
        sentence,
        &[
            "原来",
            "真正",
            "其实",
            "秘密",
            "真相",
            "身份",
            "名单",
            "规则",
            "终于明白",
            "彻底想清楚",
        ],
    )
    .min(2);
    let contrast = usize::from(has_reveal_contrast(sentence));
    let knowledge = usize::from(
        count_any(sentence, &["他知道", "她知道"]) > 0 && (explicit > 0 || contrast > 0),
    );
    let enumerated = usize::from(has_reveal_ordinal(sentence) && (explicit > 0 || contrast > 0));
    explicit + contrast + knowledge + enumerated
}

fn has_reveal_contrast(sentence: &str) -> bool {
    if !sentence.contains("不是") {
        return false;
    }
    if !(sentence.contains("而是") || sentence.contains("却是") || sentence.contains("其实是"))
    {
        return false;
    }

    count_any(
        sentence,
        &[
            "原来", "其实", "秘密", "真相", "身份", "名单", "规则", "钥匙", "封印", "用途", "名字",
            "幕后", "线索", "变化", "发现",
        ],
    ) > 0
}

fn has_reveal_ordinal(sentence: &str) -> bool {
    [
        "第一", "第二", "第三", "第四", "第五", "第六", "第七", "第八",
    ]
    .iter()
    .any(|marker| {
        sentence.find(marker).is_some_and(|index| {
            let suffix = sentence[index + marker.len()..].chars().next();
            !matches!(
                suffix,
                Some('章')
                    | Some('层')
                    | Some('块')
                    | Some('次')
                    | Some('声')
                    | Some('截')
                    | Some('炉')
                    | Some('天')
                    | Some('夜')
            )
        })
    })
}

fn has_opening_momentum(text: &str) -> bool {
    let opening: String = text.chars().take(300).collect();
    count_any(
        &opening,
        &[
            "目标",
            "任务",
            "时限",
            "准备",
            "推演",
            "试",
            "练",
            "修",
            "炼",
            "突破",
            "瓶颈",
            "养伤",
            "疗伤",
            "丹",
            "药",
            "炉",
            "火候",
            "灵石",
            "灵气",
            "禁制",
            "令牌",
            "大比",
            "试炼",
            "执事",
            "子时",
            "反噬",
            "追兵",
            "封谷",
            "雨",
            "血",
            "尸",
            "死",
            "枪",
            "刀",
            "警",
            "哭",
            "喊",
            "救",
            "逃",
            "危险",
            "异常",
            "不对劲",
            "来不及",
            "必须",
            "突然",
            "敲门",
            "电话",
        ],
    ) >= 2
}

fn has_ending_progression(text: &str) -> bool {
    let total = text.chars().count();
    let ending: String = text.chars().skip(total.saturating_sub(260)).collect();
    ending.contains('？')
        || ending.contains('?')
        || count_any(
            &ending,
            &[
                "突然",
                "不见了",
                "还活着",
                "别",
                "门外",
                "电话",
                "短信",
                "名单",
                "真相",
                "秘密",
                "下一秒",
                "身后",
                "睁开",
                "声音",
                "多出",
                "变成",
                "显出",
                "渗水",
                "笔画",
                "成型",
                "纸条",
                "地图",
                "回",
                "活",
                "来不及",
                "还在",
                "午时",
                "子时",
                "倒计时",
                "提前",
                "查验",
                "验骨",
                "封禁",
                "封死",
                "追查",
                "密报",
                "盯",
                "逼近",
                "到场",
                "命令",
                "通缉",
                "擂台",
                "大比",
                "执事房",
                "内门",
                "铜镜",
                "令牌",
                "完成",
                "成功",
                "炼成",
                "练成",
                "稳住",
                "恢复",
                "突破",
                "拿到",
                "换到",
                "保住",
                "失去",
                "付出",
                "欠下",
                "决定",
                "选择",
                "答应",
                "拒绝",
                "承诺",
                "约定",
                "条件",
                "交易",
                "人情",
                "转身",
                "收起",
                "放下",
                "握紧",
                "点头",
            ],
        ) >= 2
}

fn sensory_detail_score(text: &str) -> u8 {
    let hits = count_any(
        text,
        &[
            "雨", "风", "冷", "热", "湿", "疼", "痛", "臭", "腥", "霉", "灯", "影", "响", "声",
            "味", "指尖", "掌心", "喉咙", "呼吸",
        ],
    );
    (hits.saturating_mul(6).min(100)) as u8
}

fn professional_detail_score(text: &str) -> u8 {
    let hits = count_any(
        text,
        &[
            "手套", "编号", "登记", "记录", "流程", "工具", "报告", "档案", "监控", "警戒", "封条",
            "尸袋", "冷柜", "推车", "消毒", "签字", "证物", "现场", "灵石", "丹", "药", "药材",
            "炉", "火候", "禁制", "境界", "火脉", "外门", "内门", "执事", "大比", "符", "灵气",
            "经脉", "洞府", "功法", "法器", "令牌", "试炼", "宗门", "阵",
        ],
    );
    (hits.saturating_mul(8).min(100)) as u8
}

fn metric(label: &str, value: f64, unit: &str, target: Option<f64>) -> QualityMetric {
    QualityMetric {
        label: label.to_string(),
        value,
        unit: unit.to_string(),
        target,
    }
}

fn warning(title: &str, detail: &str, suggestion: &str) -> QualityWarning {
    QualityWarning {
        title: title.to_string(),
        detail: detail.to_string(),
        suggestion: suggestion.to_string(),
    }
}

fn verdict(score: u8, warning_count: usize) -> String {
    match (score, warning_count) {
        (86..=100, 0..=2) => "strong",
        (72..=100, 0..=4) => "usable",
        (52..=100, _) => "needs_revision",
        _ => "weak",
    }
    .to_string()
}

fn summary(score: u8, is_body: bool, warnings: &[QualityWarning]) -> String {
    if is_body {
        format!(
            "本地质量分 {}。主要风险：{}",
            score,
            warnings
                .iter()
                .take(3)
                .map(|warning| warning.title.as_str())
                .collect::<Vec<_>>()
                .join("、")
        )
    } else {
        format!(
            "本地质量分 {}。这是结构资料检查，重点看信息量和后续可执行性。",
            score
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn artifact(content: &str) -> Artifact {
        Artifact {
            id: 7,
            project_id: 1,
            chapter_id: Some(1),
            stage: "draft".to_string(),
            title: "第 1 章".to_string(),
            content: content.to_string(),
            version: 1,
            status: "pending_human_approval".to_string(),
            parent_artifact_id: None,
            created_at: "2026-01-01T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn flags_weak_opening_and_template_language() {
        let report = analyze_artifact(&artifact(
            "林远想起很多往事。\n他意识到命运像一张网一样笼罩下来。\n事实上，这说明他必须成长。\n这一切像梦一样。",
        ));

        assert!(report.score < 72);
        assert!(report
            .warnings
            .iter()
            .any(|warning| warning.title == "开篇驱动力不足"));
        assert!(report
            .warnings
            .iter()
            .any(|warning| warning.title == "模板化比喻"));
    }

    #[test]
    fn rewards_pressure_and_progression_signals() {
        let report = analyze_artifact(&artifact(
            "暴雨砸在殡仪馆铁门上，尸袋里的无名男人突然敲了一下冷柜。\n“别烧我。”\n林远的手套还沾着消毒水味，登记表上的编号却自己变成了红色。\n门外警笛逼近，监控屏幕同时黑了。\n最后一格画面里，尸体睁开眼，看向他身后：门外还有一个声音在说，名单上第一个就是林远？",
        ));

        assert!(report.score >= 60);
        assert!(report
            .metrics
            .iter()
            .any(|metric| metric.label == "开篇驱动力" && metric.value == 1.0));
    }

    #[test]
    fn accepts_quiet_growth_ending_without_danger_hook() {
        let report = analyze_artifact(&artifact(
            "天亮前，陆烬把最后一枚下品灵石压进炉脚，照着昨夜记下的火候重新运转控火诀。\n经脉的刺痛逼得他三次停手，他便把每次失控的位置刻在竹片上，再从最短的一段重新练起。\n第四次，炉中药液终于不再翻沸。陆烬稳住气息，把这一轮灵力完整送过右臂，先前始终断开的半式终于练成。\n他没有继续贪功，只把剩下的药液封好，收起竹片，决定午后先去换一份更耐火的辅材。",
        ));

        assert!(report
            .metrics
            .iter()
            .any(|metric| metric.label == "结尾功能" && metric.value == 1.0));
        assert!(!report
            .warnings
            .iter()
            .any(|warning| warning.title == "结尾功能不清"));
    }

    #[test]
    fn accepts_xianxia_development_opening() {
        let report = analyze_artifact(&artifact(
            "子时刚过，外门丹房的火脉只剩一线赤光。\n陆烬把三枚下品灵石压进炉脚，先试第七遍控火诀，掌心旧伤被灵气一冲便疼得发麻。\n他今晚必须炼出截痛丸，否则明日大比前连剑都握不稳。\n炉壁忽然浮出一道陌生禁制，像是在等他的血。\n门外执事的脚步停住，有人低声道：“韩厉师兄说，别让他撑到天亮。”",
        ));

        assert!(report
            .metrics
            .iter()
            .any(|metric| metric.label == "开篇驱动力" && metric.value == 1.0));
        assert!(!report
            .warnings
            .iter()
            .any(|warning| warning.title == "开篇驱动力不足"));
    }

    #[test]
    fn flags_dense_reveal_dump() {
        let report = analyze_artifact(&artifact(
            "雨夜里，林远拿到名单，真正的规则终于露出来。\n第一，旧案不是失踪，而是献祭。第二，药堂不是救人，而是筛人。第三，师兄的身份其实是内门暗线。\n他知道原来钥匙不是钥匙，而是封印。她知道名单不是名单，而是下一批引子。\n真正的秘密、真相、身份和规则同时压下来，林远彻底想清楚自己必须去大比前夜抢一枚令牌。\n门外突然传来脚步声，纸条背面多出一行字：名单下一个名字，是他。",
        ));

        assert!(report
            .warnings
            .iter()
            .any(|warning| warning.title == "信息反转偏密"));
    }

    #[test]
    fn long_chapter_gets_more_reveal_tolerance() {
        let long_padding =
            "陆烬沿着石阶一寸寸往下摸，先试炉壁温差，再记脚下碎石的方向。".repeat(110);
        let report = analyze_artifact(&artifact(&format!(
            "{}\n第一层旧规露出来。第二层身份误差也露出来。第三个真相是黑牌会吃火。第四个真相是血书不是血书。第五个线索是出口会动。第六个发现是有人提前来过。第七个变化是炉底不是空的。第八个变化是门后还有门。\n最后，他听见炉门后传来一声轻响。",
            long_padding
        )));

        assert!(!report
            .warnings
            .iter()
            .any(|warning| warning.title == "信息反转偏密"));
    }

    #[test]
    fn does_not_count_scene_clarifications_as_reveal_dump() {
        let report = analyze_artifact(&artifact(
            "赵执事不是要罚他——是要把他钉死在炉前，哪儿也去不了。\n不是天然裂缝。里面不是矿道。不是他主动点燃。\n石壁上开始出现手印——不是掌印，是五指抠进铁浆壳抓出的印子。\n他手里那块黑牌边缘沾着一层暗红色粉末——不是炉灰，是干涸的血粉末。\n最后，他听见炉门外的铁链忽然绷紧。",
        ));

        assert!(!report
            .warnings
            .iter()
            .any(|warning| warning.title == "信息反转偏密"));
    }

    #[test]
    fn flags_abstract_and_outsourced_costs_in_setting() {
        let mut setting = artifact(
            "核心规则：气运会被抽走，因果会反噬，天命也会折损。\n常规修炼依赖替命之躯和代价转嫁；必要时可以牺牲同伴继续突破。",
        );
        setting.stage = "setting".to_string();
        setting.chapter_id = None;

        let report = analyze_artifact(&setting);
        assert!(report
            .warnings
            .iter()
            .any(|warning| warning.title == "代价过于抽象"));
        assert!(report
            .warnings
            .iter()
            .any(|warning| warning.title == "成长代价过度外包"));
        assert!(report.score < 72);
    }

    #[test]
    fn accepts_setting_that_explicitly_bans_abstract_or_outsourced_costs() {
        let mut setting = artifact(
            "古轴看不到气运和因果，也不得把代价转嫁给他人。\n常规修炼只消耗主角自己的伤势、资源和时间；错位支付只限已经受损的身体部位。",
        );
        setting.stage = "setting".to_string();
        setting.chapter_id = None;

        let report = analyze_artifact(&setting);
        assert!(!report
            .warnings
            .iter()
            .any(|warning| warning.title == "代价过于抽象"));
        assert!(!report
            .warnings
            .iter()
            .any(|warning| warning.title == "成长代价过度外包"));
    }
}
