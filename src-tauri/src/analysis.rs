use crate::{
    models::FileAnalysis,
    storage::{now_string, path_to_string},
};
use calamine::{open_workbook_auto, Reader};
use encoding_rs::{GBK, UTF_16BE, UTF_16LE};
use quick_xml::events::Event;
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

const MAX_TEXT_BYTES: usize = 64 * 1024 * 1024;
const MAX_EXTRACTED_CHARS: usize = 1_000_000;
const MAX_OCR_PAGES: u32 = 40;

#[derive(Debug, Clone)]
pub struct ParsedContent {
    pub status: String,
    pub summary: String,
    pub sections: Vec<String>,
    pub recommended_category: String,
    pub full_text: String,
    pub parser: String,
    pub page_count: Option<u32>,
    pub sheet_count: Option<u32>,
    pub row_count: Option<u64>,
    pub column_count: Option<u64>,
    pub warnings: Vec<String>,
    pub document_type: String,
    pub business_domain: String,
    pub business_purpose: String,
    pub business_summary: String,
    pub technical_detail: String,
}

impl ParsedContent {
    fn with_business_semantics(mut self, file_name: &str) -> Self {
        let (document_type, domain, purpose, summary) =
            infer_business_semantics(file_name, &self.full_text, &self.recommended_category);
        self.document_type = document_type;
        self.business_domain = domain;
        self.business_purpose = purpose;
        self.business_summary = summary;
        self
    }
}

/// 业务语义识别（规则优先）。返回 (documentType, businessDomain, businessPurpose, businessSummary)。
/// 不依赖 DeepSeek；DeepSeek 最小上下文复核由调用方在 refine 阶段可选执行。
pub fn infer_business_semantics(
    file_name: &str,
    text: &str,
    fallback_category: &str,
) -> (String, String, String, String) {
    let haystack = format!("{}\n{}", file_name, text);
    let haystack_lower = haystack.to_lowercase();
    let has = |keywords: &[&str]| keywords.iter().any(|kw| haystack_lower.contains(kw));

    // 优先级从具体到通用。第一条命中即为 documentType。
    if has(&["vave", "降本", "成本改善", "立项申请", "申请单", "申请表"]) {
        return (
            "VAVE/降本申请单".to_string(),
            "研发/质量/降本".to_string(),
            "VAVE 申请流程与审批".to_string(),
            "识别为降本/申请类资料，包含申请流程相关字段。".to_string(),
        );
    }
    if has(&[
        "周计划",
        "周工作",
        "月计划",
        "周报",
        "工作安排",
        "排班",
        "值班",
        "任务计划",
    ]) {
        return (
            "工作计划".to_string(),
            "人员/部门管理".to_string(),
            "人员与部门工作安排".to_string(),
            "识别为工作计划类资料，描述时间与任务安排。".to_string(),
        );
    }
    if has(&[
        "订单",
        "工单",
        "排产",
        "生产计划",
        "报工",
        "在制",
        "完工",
        "交期",
        "车间",
    ]) {
        return (
            "车间生产/订单报表".to_string(),
            "制造/生产".to_string(),
            "订单进度与完工状态跟踪".to_string(),
            "识别为生产制造类报表，涉及订单/工序/进度信息。".to_string(),
        );
    }
    if has(&[
        "报废",
        "不良",
        "合格率",
        "良率",
        "质检",
        "检验",
        "抽检",
        "缺陷",
        "不良率",
    ]) {
        return (
            "质量统计报表".to_string(),
            "质量".to_string(),
            "质量指标统计与缺陷跟踪".to_string(),
            "识别为质量类报表，包含报废/不良/合格率等字段。".to_string(),
        );
    }
    if has(&["发票", "报销", "付款", "应付", "对账", "费用", "预算"]) {
        return (
            "财务费用报表".to_string(),
            "财务".to_string(),
            "费用与资金往来记录".to_string(),
            "识别为财务类报表，涉及费用/发票/对账信息。".to_string(),
        );
    }
    if has(&[
        "考勤", "请假", "人事", "员工", "入职", "薪资", "薪酬", "加班",
    ]) {
        return (
            "人事考勤报表".to_string(),
            "人事".to_string(),
            "人员考勤与人事信息管理".to_string(),
            "识别为人事考勤类资料，涉及人员与考勤信息。".to_string(),
        );
    }
    if has(&["库存", "物料", "出入库", "领料", "盘点", "采购单", "送货单"]) {
        return (
            "库存/物料报表".to_string(),
            "供应链/库存".to_string(),
            "物料与库存流转跟踪".to_string(),
            "识别为库存物料类报表，涉及出入库/物料信息。".to_string(),
        );
    }
    if has(&["培训", "课程", "学时", "讲师"]) {
        return (
            "培训记录".to_string(),
            "培训".to_string(),
            "培训计划与记录".to_string(),
            "识别为培训类资料。".to_string(),
        );
    }
    if has(&["需求", "prd", "方案", "架构", "设计文档", "立项"]) {
        return (
            "需求/方案文档".to_string(),
            "研发".to_string(),
            "需求与方案设计".to_string(),
            "识别为需求或方案类文档。".to_string(),
        );
    }
    if has(&["测试", "用例", "验收", "测试报告"]) {
        return (
            "测试文档".to_string(),
            "质量/测试".to_string(),
            "测试用例与验收".to_string(),
            "识别为测试类文档。".to_string(),
        );
    }
    if has(&["会议", "纪要", "会议记录", "minutes"]) {
        return (
            "会议记录".to_string(),
            "通用办公".to_string(),
            "会议内容记录".to_string(),
            "识别为会议记录类资料。".to_string(),
        );
    }
    if has(&["排产表", "进度", "计划表", "甘特", "里程碑"]) {
        return (
            "项目计划表".to_string(),
            "项目管理".to_string(),
            "项目进度计划".to_string(),
            "识别为项目计划类报表。".to_string(),
        );
    }

    // 兜底：按原分类给一个保守语义
    let document_type = match fallback_category {
        "数据表格" => "数据表格",
        "方案演示" => "方案演示文稿",
        "会议记录" => "会议记录",
        "提示词" => "提示词资料",
        "测试资料" => "测试资料",
        "交付文件" => "交付资料",
        "参考资料" => "参考资料",
        "开发文件" => "开发资料",
        "需求文档" | "项目资料" => "项目资料",
        other => {
            if other.is_empty() {
                "通用资料"
            } else {
                other
            }
        }
    };
    (
        document_type.to_string(),
        "未分类".to_string(),
        "待进一步确认用途".to_string(),
        format!("未能识别明确业务语义，按“{document_type}”归置。"),
    )
}

/// 判断资料在工作流中的用途。用途与业务主题分开，避免截图中的关键词被当成正式业务单据。
pub fn infer_document_purpose(
    file_name: &str,
    file_type: &str,
    text: &str,
    sections: &[String],
) -> (String, u8, Vec<String>) {
    let name = file_name.to_lowercase();
    let body = text.to_lowercase();
    let structure = sections.join("\n").to_lowercase();
    let has_any = |value: &str, terms: &[&str]| terms.iter().any(|term| value.contains(term));
    let mut evidence = Vec::new();

    if matches!(file_type, "png" | "jpg" | "jpeg" | "bmp" | "tif" | "tiff") {
        evidence.push("文件类型为图片，内容仅作为截图或视觉证据。".to_string());
        if has_any(&name, &["截图", "screen", "capture", "snip"]) {
            evidence.push("文件名包含截图标识。".to_string());
        }
        return ("evidence".to_string(), 94, evidence);
    }
    if has_any(&name, &["template", "模板", "导入样表", "样表"]) {
        evidence.push("文件名明确包含模板标识。".to_string());
        return ("template".to_string(), 92, evidence);
    }
    if has_any(
        &name,
        &[
            "开发状态",
            "进展",
            "完成报告",
            "测试报告",
            "验收结果",
            "总结",
            "复盘",
            "周报",
        ],
    ) || has_any(
        &structure,
        &["完成情况", "当前状态", "测试结果", "遗留问题"],
    ) {
        evidence.push("文件名或结构体现状态、结果或总结用途。".to_string());
        return ("report".to_string(), 88, evidence);
    }
    let scheduling_signals = [
        "负责人",
        "开始日期",
        "结束日期",
        "截止日期",
        "计划完成",
        "排期",
        "里程碑",
    ]
    .iter()
    .filter(|term| body.contains(**term) || structure.contains(**term))
    .count();
    if has_any(&name, &["计划", "排期", "schedule", "roadmap"]) && scheduling_signals >= 2 {
        evidence.push("文件名包含计划标识。".to_string());
        evidence.push(format!("表头或正文命中 {scheduling_signals} 个安排字段。"));
        return (
            "plan".to_string(),
            (78 + scheduling_signals.min(4) * 4) as u8,
            evidence,
        );
    }
    if has_any(&name, &["需求", "prd", "规格", "需求说明"])
        || has_any(&structure, &["需求范围", "验收标准", "功能要求"])
    {
        evidence.push("文件名或章节包含明确需求结构。".to_string());
        return ("requirement".to_string(), 86, evidence);
    }
    if has_any(&name, &["会议纪要", "沟通记录", "邮件", "通知"])
        || has_any(&structure, &["参会人", "会议议题", "抄送"])
    {
        evidence.push("文件名或结构体现沟通记录用途。".to_string());
        return ("communication".to_string(), 84, evidence);
    }
    if matches!(file_type, "xls" | "xlsx" | "xlsb" | "ods" | "csv" | "tsv") {
        evidence.push("文件为结构化表格，未发现足够的计划或报告证据。".to_string());
        return ("data".to_string(), 72, evidence);
    }
    if has_any(&name, &["参考", "说明", "手册", "指南"]) {
        evidence.push("文件名体现参考资料用途。".to_string());
        return ("reference".to_string(), 76, evidence);
    }
    (
        "unknown".to_string(),
        30,
        vec!["现有证据不足以确认资料用途。".to_string()],
    )
}

pub fn analyze_managed_file(
    file_id: &str,
    file_name: &str,
    path: &Path,
    file_type: &str,
    content_hash: &str,
) -> (FileAnalysis, Option<String>) {
    let extracted_at = now_string();
    let parsed = match file_type {
        "txt" | "md" => parse_text_file(path),
        "csv" => parse_delimited_file(path, ','),
        "tsv" => parse_delimited_file(path, '\t'),
        "docx" => parse_docx_file(path),
        "xls" | "xlsx" | "xlsb" | "ods" => parse_workbook_file(path),
        "pptx" => parse_pptx_file(path),
        "pdf" => parse_pdf_file(path),
        "png" | "jpg" | "jpeg" | "bmp" | "tif" | "tiff" => parse_image_file(path),
        "ppt" => Err("旧版 .ppt 暂无可靠解析器，已进入待检查事项。".to_string()),
        "doc" => Err("旧版 .doc 暂无可靠解析器，已进入待检查事项。".to_string()),
        _ => Err(format!(
            "暂不支持解析 .{file_type} 文件，已进入待检查事项。"
        )),
    };

    match parsed {
        Ok(mut content) => {
            content.full_text = truncate_text(&content.full_text, MAX_EXTRACTED_CHARS);
            content = content.with_business_semantics(file_name);
            let (document_purpose, document_purpose_confidence, document_purpose_evidence) =
                infer_document_purpose(file_name, file_type, &content.full_text, &content.sections);
            if document_purpose == "evidence" {
                content.document_type = "证据截图".to_string();
                content.business_domain = "待依据项目确认".to_string();
                content.business_purpose = "记录界面或现场证据".to_string();
                content.business_summary =
                    "识别为截图证据，不依据截图中的单个业务词直接判定单据类型。".to_string();
            } else if document_purpose == "report" && content.document_type == "工作计划" {
                content.document_type = "开发状态报告".to_string();
                content.business_purpose = "记录进展、结果与遗留事项".to_string();
                content.business_summary = "识别为状态或结果报告，不按计划文件处理。".to_string();
            }
            let analysis = FileAnalysis {
                file_id: file_id.to_string(),
                file_name: file_name.to_string(),
                managed_path: path_to_string(path),
                parse_status: content.status,
                content_summary: content.summary,
                main_fields_or_sections: content.sections,
                recommended_category: content.recommended_category,
                parse_failure_reason: String::new(),
                extracted_at,
                parser: content.parser,
                content_hash: content_hash.to_string(),
                extracted_text_path: String::new(),
                page_count: content.page_count,
                sheet_count: content.sheet_count,
                row_count: content.row_count,
                column_count: content.column_count,
                warnings: content.warnings,
                document_type: content.document_type,
                business_domain: content.business_domain,
                business_purpose: content.business_purpose,
                business_summary: content.business_summary,
                document_purpose,
                document_purpose_confidence,
                document_purpose_evidence,
                technical_detail: content.technical_detail,
            };
            (analysis, Some(content.full_text))
        }
        Err(reason) => {
            let (friendly_reason, technical_detail) = friendly_parse_error(&reason);
            (
                FileAnalysis {
                    file_id: file_id.to_string(),
                    file_name: file_name.to_string(),
                    managed_path: path_to_string(path),
                    parse_status: if matches!(file_type, "ppt" | "doc") {
                        "review_required".to_string()
                    } else {
                        "failed".to_string()
                    },
                    content_summary: "未能提取可用正文。".to_string(),
                    main_fields_or_sections: Vec::new(),
                    recommended_category: "待检查".to_string(),
                    parse_failure_reason: friendly_reason,
                    extracted_at,
                    parser: "none".to_string(),
                    content_hash: content_hash.to_string(),
                    extracted_text_path: String::new(),
                    page_count: None,
                    sheet_count: None,
                    row_count: None,
                    column_count: None,
                    warnings: Vec::new(),
                    document_type: String::new(),
                    business_domain: String::new(),
                    business_purpose: String::new(),
                    business_summary: String::new(),
                    document_purpose: "unknown".to_string(),
                    document_purpose_confidence: 0,
                    document_purpose_evidence: vec![
                        "文件解析失败，无法可靠判断资料用途。".to_string()
                    ],
                    technical_detail,
                },
                None,
            )
        }
    }
}

/// 解析失败错误友好化：原始错误（如 Windows OCR 的 HRESULT）保留在 technical_detail，
/// 返回给 UI 展示的是用户可理解的说明。
fn friendly_parse_error(raw: &str) -> (String, String) {
    let trimmed = raw.trim();
    if trimmed.contains("OCR") || trimmed.contains("0x80004005") {
        (
            "未能从文件中识别出文字，可重试或手动归类。".to_string(),
            trimmed.to_string(),
        )
    } else {
        (trimmed.to_string(), String::new())
    }
}

pub fn parse_text_file(path: &Path) -> Result<ParsedContent, String> {
    let bytes = read_bounded(path)?;
    let (text, encoding) = decode_text(&bytes)?;
    let mut parsed = parsed_from_text(&text, "需求文档", "text")?;
    parsed.sections.insert(0, format!("文本编码：{encoding}"));
    Ok(parsed)
}

pub fn parse_delimited_file(path: &Path, delimiter: char) -> Result<ParsedContent, String> {
    let bytes = read_bounded(path)?;
    let (text, encoding) = decode_text(&bytes)?;
    let rows = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| parse_delimited_line(line, delimiter))
        .collect::<Vec<_>>();
    if rows.is_empty() {
        return Err("表格文本中没有可用行。".to_string());
    }
    let column_count = rows.iter().map(Vec::len).max().unwrap_or_default();
    let headers = rows[0]
        .iter()
        .take(16)
        .filter(|value| !value.trim().is_empty())
        .cloned()
        .collect::<Vec<_>>();
    let samples = rows
        .iter()
        .skip(1)
        .take(5)
        .map(|row| row.iter().take(10).cloned().collect::<Vec<_>>().join(" | "))
        .filter(|row| !row.trim().is_empty())
        .collect::<Vec<_>>();
    let mut sections = vec![
        format!("文本编码：{encoding}"),
        format!(
            "表头：{}",
            if headers.is_empty() {
                "未识别".to_string()
            } else {
                headers.join("、")
            }
        ),
    ];
    sections.extend(samples.iter().map(|sample| format!("样例：{sample}")));
    Ok(ParsedContent {
        status: "success".to_string(),
        summary: format!(
            "识别到 {} 行、{} 列。{}",
            rows.len(),
            column_count,
            samples
                .first()
                .map(|value| format!("首条样例：{}", truncate_text(value, 100)))
                .unwrap_or_else(|| "没有数据样例。".to_string())
        ),
        sections,
        recommended_category: "数据表格".to_string(),
        full_text: text,
        parser: if delimiter == '\t' { "tsv" } else { "csv" }.to_string(),
        page_count: None,
        sheet_count: Some(1),
        row_count: Some(rows.len() as u64),
        column_count: Some(column_count as u64),
        warnings: Vec::new(),
        document_type: String::new(),
        business_domain: String::new(),
        business_purpose: String::new(),
        business_summary: String::new(),
        technical_detail: String::new(),
    })
}

pub fn parse_docx_file(path: &Path) -> Result<ParsedContent, String> {
    let file = fs::File::open(path).map_err(|err| format!("打开 docx 失败：{err}"))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|err| format!("读取 docx 压缩结构失败：{err}"))?;
    let document = read_zip_text(&mut archive, "word/document.xml", "docx 正文")?;
    let (paragraphs, table_rows) = parse_word_document_xml(&document)?;
    let mut full_parts = paragraphs.clone();
    full_parts.extend(table_rows.iter().map(|row| row.join(" | ")));
    if full_parts.is_empty() {
        return Err("docx 中没有提取到段落或表格文本。".to_string());
    }

    let mut sections = paragraphs
        .iter()
        .take(8)
        .map(|paragraph| format!("段落：{}", truncate_text(paragraph, 100)))
        .collect::<Vec<_>>();
    sections.extend(
        table_rows
            .iter()
            .take(5)
            .map(|row| format!("表格：{}", truncate_text(&row.join(" | "), 120))),
    );
    let full_text = full_parts.join("\n");
    Ok(ParsedContent {
        status: "success".to_string(),
        summary: format!(
            "识别到 {} 个段落、{} 行表格内容。{}",
            paragraphs.len(),
            table_rows.len(),
            summarize_text(&full_text)
        ),
        sections,
        recommended_category: infer_document_category(&full_text),
        full_text,
        parser: "docx-openxml".to_string(),
        page_count: None,
        sheet_count: None,
        row_count: Some(table_rows.len() as u64),
        column_count: table_rows
            .iter()
            .map(Vec::len)
            .max()
            .map(|value| value as u64),
        warnings: Vec::new(),
        document_type: String::new(),
        business_domain: String::new(),
        business_purpose: String::new(),
        business_summary: String::new(),
        technical_detail: String::new(),
    })
}

pub fn parse_workbook_file(path: &Path) -> Result<ParsedContent, String> {
    let mut workbook = open_workbook_auto(path).map_err(|err| format!("打开工作簿失败：{err}"))?;
    let sheet_names = workbook.sheet_names().to_owned();
    if sheet_names.is_empty() {
        return Err("工作簿中没有工作表。".to_string());
    }

    let mut sections = Vec::new();
    let mut full_text = Vec::new();
    let mut total_rows = 0_u64;
    let mut max_columns = 0_u64;
    let mut warnings = Vec::new();
    for sheet_name in &sheet_names {
        match workbook.worksheet_range(sheet_name) {
            Ok(range) => {
                let (height, width) = range.get_size();
                total_rows += height as u64;
                max_columns = max_columns.max(width as u64);
                let headers = range
                    .rows()
                    .next()
                    .map(|row| {
                        row.iter()
                            .take(24)
                            .map(ToString::to_string)
                            .filter(|value| !value.trim().is_empty())
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                let formulas = workbook
                    .worksheet_formula(sheet_name)
                    .map(|formula_range| {
                        formula_range
                            .rows()
                            .flatten()
                            .filter(|value| !value.trim().is_empty())
                            .count()
                    })
                    .unwrap_or_default();
                sections.push(format!(
                    "{sheet_name}：{height} 行 × {width} 列；表头：{}；公式：{formulas}",
                    if headers.is_empty() {
                        "未识别".to_string()
                    } else {
                        headers.join("、")
                    }
                ));
                for row in range.rows().take(8) {
                    let values = row
                        .iter()
                        .take(16)
                        .map(ToString::to_string)
                        .collect::<Vec<_>>();
                    if values.iter().any(|value| !value.trim().is_empty()) {
                        full_text.push(format!("{sheet_name} | {}", values.join(" | ")));
                    }
                }
            }
            Err(err) => warnings.push(format!("工作表 {sheet_name} 读取失败：{err}")),
        }
    }
    if sections.is_empty() {
        return Err(format!("所有工作表读取失败：{}", warnings.join("；")));
    }
    let status = if warnings.is_empty() {
        "success"
    } else {
        "partial"
    };
    let workbook_text = full_text.join("\n");
    let (document_type, business_domain, business_purpose, business_summary) =
        infer_business_semantics(
            &path
                .file_stem()
                .and_then(|name| name.to_str())
                .unwrap_or(""),
            &workbook_text,
            "数据表格",
        );
    let base_summary = format!(
        "识别到 {} 个工作表，共约 {} 行，最大 {} 列。{}",
        sheet_names.len(),
        total_rows,
        max_columns,
        workbook_text
            .lines()
            .next()
            .map(|value| format!("样例：{}", truncate_text(value, 120)))
            .unwrap_or_else(|| "未提取到非空样例。".to_string())
    );
    Ok(ParsedContent {
        status: status.to_string(),
        summary: if document_type == "数据表格" || document_type == "通用资料" {
            base_summary
        } else {
            format!("{document_type}：{base_summary}")
        },
        sections,
        recommended_category: "数据表格".to_string(),
        full_text: workbook_text,
        parser: "calamine".to_string(),
        page_count: None,
        sheet_count: Some(sheet_names.len() as u32),
        row_count: Some(total_rows),
        column_count: Some(max_columns),
        warnings,
        document_type,
        business_domain,
        business_purpose,
        business_summary,
        technical_detail: String::new(),
    })
}

pub fn parse_pptx_file(path: &Path) -> Result<ParsedContent, String> {
    let file = fs::File::open(path).map_err(|err| format!("打开 pptx 失败：{err}"))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|err| format!("读取 pptx 压缩结构失败：{err}"))?;
    let mut slide_names = (0..archive.len())
        .filter_map(|index| {
            archive
                .by_index(index)
                .ok()
                .map(|entry| entry.name().to_string())
        })
        .filter(|name| is_numbered_openxml_part(name, "ppt/slides/slide", ".xml"))
        .collect::<Vec<_>>();
    slide_names.sort_by_key(|name| numbered_part_index(name));
    if slide_names.is_empty() {
        return Err("pptx 中没有找到幻灯片。".to_string());
    }

    let mut sections = Vec::new();
    let mut full_text = Vec::new();
    for (index, slide_name) in slide_names.iter().enumerate() {
        let xml = read_zip_text(&mut archive, slide_name, "pptx 幻灯片")?;
        let text = extract_openxml_text(&xml)?;
        if !text.trim().is_empty() {
            sections.push(format!(
                "第 {} 页：{}",
                index + 1,
                truncate_text(&text.replace('\n', " "), 140)
            ));
            full_text.push(format!("第 {} 页\n{text}", index + 1));
        } else {
            sections.push(format!("第 {} 页：未识别到文字", index + 1));
        }
    }
    let joined = full_text.join("\n\n");
    Ok(ParsedContent {
        status: "success".to_string(),
        summary: format!(
            "识别到 {} 页幻灯片。{}",
            slide_names.len(),
            if joined.trim().is_empty() {
                "未提取到文字，可能以图片为主。".to_string()
            } else {
                summarize_text(&joined)
            }
        ),
        sections,
        recommended_category: "方案演示".to_string(),
        full_text: joined,
        parser: "pptx-openxml".to_string(),
        page_count: Some(slide_names.len() as u32),
        sheet_count: None,
        row_count: None,
        column_count: None,
        warnings: Vec::new(),
        document_type: String::new(),
        business_domain: String::new(),
        business_purpose: String::new(),
        business_summary: String::new(),
        technical_detail: String::new(),
    })
}

pub fn parse_pdf_file(path: &Path) -> Result<ParsedContent, String> {
    let document =
        lopdf::Document::load(path).map_err(|err| format!("读取 PDF 结构失败：{err}"))?;
    let page_count = document.get_pages().len() as u32;
    let extracted = pdf_extract::extract_text(path).unwrap_or_default();
    if meaningful_char_count(&extracted) >= 20 {
        let mut parsed = parsed_from_text(&extracted, "需求文档", "pdf-text")?;
        parsed.page_count = Some(page_count);
        parsed.sections.insert(0, format!("页数：{page_count}"));
        return Ok(parsed);
    }

    #[cfg(target_os = "windows")]
    {
        let (text, processed_pages, mut warnings) = ocr_pdf(path, page_count)?;
        if processed_pages < page_count {
            warnings.push(format!(
                "为避免长时间无响应，本次 OCR 处理前 {processed_pages} 页，共 {page_count} 页。"
            ));
        }
        let mut parsed = parsed_from_text(&text, "需求文档", "windows-ocr-pdf")?;
        parsed.page_count = Some(page_count);
        parsed.status = if warnings.is_empty() {
            "success".to_string()
        } else {
            "partial".to_string()
        };
        parsed.warnings = warnings;
        parsed.sections.insert(
            0,
            format!("页数：{page_count}；OCR 页数：{processed_pages}"),
        );
        Ok(parsed)
    }

    #[cfg(not(target_os = "windows"))]
    Err(format!(
        "PDF 共 {page_count} 页，但没有可提取正文；当前平台未提供扫描 PDF OCR。"
    ))
}

pub fn parse_image_file(path: &Path) -> Result<ParsedContent, String> {
    #[cfg(target_os = "windows")]
    {
        let text = ocr_image(path)?;
        let mut parsed = parsed_from_text(&text, "图片资料", "windows-ocr-image")?;
        parsed.sections.insert(0, "图片 OCR 文字".to_string());
        Ok(parsed)
    }

    #[cfg(not(target_os = "windows"))]
    Err("当前平台未提供图片 OCR。".to_string())
}

fn read_bounded(path: &Path) -> Result<Vec<u8>, String> {
    let metadata = fs::metadata(path).map_err(|err| format!("读取文件信息失败：{err}"))?;
    if metadata.len() > MAX_TEXT_BYTES as u64 {
        return Err(format!(
            "文件大小 {} MB，超过本地文本解析上限 {} MB。",
            metadata.len() / 1024 / 1024,
            MAX_TEXT_BYTES / 1024 / 1024
        ));
    }
    fs::read(path).map_err(|err| format!("读取文件失败：{err}"))
}

fn decode_text(bytes: &[u8]) -> Result<(String, &'static str), String> {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8(bytes[3..].to_vec())
            .map(|value| (value, "UTF-8 BOM"))
            .map_err(|err| format!("UTF-8 BOM 文本解码失败：{err}"));
    }
    if bytes.starts_with(&[0xFF, 0xFE]) {
        let (text, _, had_errors) = UTF_16LE.decode(&bytes[2..]);
        if !had_errors {
            return Ok((text.into_owned(), "UTF-16LE"));
        }
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        let (text, _, had_errors) = UTF_16BE.decode(&bytes[2..]);
        if !had_errors {
            return Ok((text.into_owned(), "UTF-16BE"));
        }
    }
    if let Ok(text) = std::str::from_utf8(bytes) {
        return Ok((text.to_string(), "UTF-8"));
    }
    let (text, _, had_errors) = GBK.decode(bytes);
    if !had_errors {
        return Ok((text.into_owned(), "GBK/GB18030"));
    }
    Err("无法按 UTF-8、UTF-16 或 GBK/GB18030 解码文本。".to_string())
}

fn parse_delimited_line(line: &str, delimiter: char) -> Vec<String> {
    let mut values = Vec::new();
    let mut current = String::new();
    let mut chars = line.chars().peekable();
    let mut quoted = false;
    while let Some(ch) = chars.next() {
        if ch == '"' {
            if quoted && chars.peek() == Some(&'"') {
                current.push('"');
                chars.next();
            } else {
                quoted = !quoted;
            }
        } else if ch == delimiter && !quoted {
            values.push(current.trim().to_string());
            current.clear();
        } else {
            current.push(ch);
        }
    }
    values.push(current.trim().to_string());
    values
}

fn parse_word_document_xml(xml: &str) -> Result<(Vec<String>, Vec<Vec<String>>), String> {
    let mut reader = quick_xml::Reader::from_str(xml);
    reader.config_mut().trim_text(false);
    let mut paragraphs = Vec::new();
    let mut table_rows = Vec::new();
    let mut current_paragraph = String::new();
    let mut current_cell = String::new();
    let mut current_row = Vec::new();
    let mut in_text = false;
    let mut in_cell = false;

    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) => match local_name(event.name().as_ref()) {
                b"t" => in_text = true,
                b"tc" => {
                    in_cell = true;
                    current_cell.clear();
                }
                b"tr" => current_row.clear(),
                _ => {}
            },
            Ok(Event::Text(event)) if in_text => {
                let value = event
                    .unescape()
                    .map_err(|err| format!("解析 docx 文本失败：{err}"))?;
                if in_cell {
                    current_cell.push_str(&value);
                } else {
                    current_paragraph.push_str(&value);
                }
            }
            Ok(Event::End(event)) => match local_name(event.name().as_ref()) {
                b"t" => in_text = false,
                b"p" => {
                    let value = current_paragraph.trim();
                    if !value.is_empty() {
                        paragraphs.push(value.to_string());
                    }
                    current_paragraph.clear();
                }
                b"tc" => {
                    current_row.push(current_cell.trim().to_string());
                    current_cell.clear();
                    in_cell = false;
                }
                b"tr" => {
                    if current_row.iter().any(|value| !value.is_empty()) {
                        table_rows.push(current_row.clone());
                    }
                    current_row.clear();
                }
                _ => {}
            },
            Ok(Event::Eof) => break,
            Err(err) => return Err(format!("解析 docx XML 失败：{err}")),
            _ => {}
        }
    }
    Ok((paragraphs, table_rows))
}

fn extract_openxml_text(xml: &str) -> Result<String, String> {
    let mut reader = quick_xml::Reader::from_str(xml);
    reader.config_mut().trim_text(false);
    let mut result = Vec::new();
    let mut in_text = false;
    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) if local_name(event.name().as_ref()) == b"t" => in_text = true,
            Ok(Event::Text(event)) if in_text => {
                let value = event
                    .unescape()
                    .map_err(|err| format!("解析 OpenXML 文本失败：{err}"))?;
                if !value.trim().is_empty() {
                    result.push(value.trim().to_string());
                }
            }
            Ok(Event::End(event)) if local_name(event.name().as_ref()) == b"t" => in_text = false,
            Ok(Event::Eof) => break,
            Err(err) => return Err(format!("解析 OpenXML 失败：{err}")),
            _ => {}
        }
    }
    Ok(result.join("\n"))
}

fn local_name(name: &[u8]) -> &[u8] {
    name.rsplit(|value| *value == b':').next().unwrap_or(name)
}

fn read_zip_text(
    archive: &mut zip::ZipArchive<fs::File>,
    name: &str,
    label: &str,
) -> Result<String, String> {
    let mut item = archive
        .by_name(name)
        .map_err(|err| format!("读取 {label} 失败：{err}"))?;
    let mut text = String::new();
    item.read_to_string(&mut text)
        .map_err(|err| format!("读取 {label} XML 失败：{err}"))?;
    Ok(text)
}

fn is_numbered_openxml_part(name: &str, prefix: &str, suffix: &str) -> bool {
    name.strip_prefix(prefix)
        .and_then(|rest| rest.strip_suffix(suffix))
        .is_some_and(|number| number.chars().all(|ch| ch.is_ascii_digit()))
}

fn numbered_part_index(name: &str) -> u32 {
    name.chars()
        .filter(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .unwrap_or(u32::MAX)
}

fn parsed_from_text(
    text: &str,
    default_category: &str,
    parser: &str,
) -> Result<ParsedContent, String> {
    let normalized = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    if normalized.is_empty() {
        return Err("没有提取到可用正文。".to_string());
    }
    let joined = normalized.join("\n");
    Ok(ParsedContent {
        status: "success".to_string(),
        summary: summarize_text(&joined),
        sections: normalized
            .iter()
            .take(10)
            .map(|line| truncate_text(line, 100))
            .collect(),
        recommended_category: if default_category == "需求文档" {
            infer_document_category(&joined)
        } else {
            default_category.to_string()
        },
        full_text: joined,
        parser: parser.to_string(),
        page_count: None,
        sheet_count: None,
        row_count: None,
        column_count: None,
        warnings: Vec::new(),
        document_type: String::new(),
        business_domain: String::new(),
        business_purpose: String::new(),
        business_summary: String::new(),
        technical_detail: String::new(),
    })
}

fn infer_document_category(text: &str) -> String {
    let lower = text.to_ascii_lowercase();
    if lower.contains("prompt") || text.contains("提示词") {
        "提示词".to_string()
    } else if text.contains("测试") || text.contains("验收") {
        "测试资料".to_string()
    } else if text.contains("需求") || lower.contains("prd") || text.contains("目标") {
        "需求文档".to_string()
    } else if text.contains("方案") || text.contains("架构") {
        "方案文档".to_string()
    } else {
        "项目资料".to_string()
    }
}

fn summarize_text(text: &str) -> String {
    truncate_text(&text.replace('\n', " "), 240)
}

fn truncate_text(text: &str, max_chars: usize) -> String {
    let mut result = text.chars().take(max_chars).collect::<String>();
    if text.chars().count() > max_chars {
        result.push('…');
    }
    result
}

fn meaningful_char_count(text: &str) -> usize {
    text.chars().filter(|ch| !ch.is_whitespace()).count()
}

#[cfg(target_os = "windows")]
fn ocr_image(path: &Path) -> Result<String, String> {
    use windows::{
        core::HSTRING, Graphics::Imaging::BitmapDecoder, Media::Ocr::OcrEngine,
        Storage::StorageFile,
    };

    with_winrt(|| {
        let file =
            StorageFile::GetFileFromPathAsync(&HSTRING::from(path_to_string(path)))?.get()?;
        let stream = file.OpenReadAsync()?.get()?;
        let decoder = BitmapDecoder::CreateAsync(&stream)?.get()?;
        validate_ocr_dimensions(decoder.PixelWidth()?, decoder.PixelHeight()?)?;
        let bitmap = decoder.GetSoftwareBitmapAsync()?.get()?;
        let engine = OcrEngine::TryCreateFromUserProfileLanguages()?;
        let result = engine.RecognizeAsync(&bitmap)?.get()?;
        let text = normalize_ocr_text(&result.Text()?.to_string_lossy());
        if meaningful_char_count(&text) == 0 {
            return Err(windows::core::Error::new(
                windows::core::HRESULT(0x80004005_u32 as i32),
                "Windows OCR 未识别到文字",
            ));
        }
        Ok(text)
    })
    .map_err(|err| format!("图片 OCR 失败：{err}"))
}

#[cfg(target_os = "windows")]
fn ocr_pdf(path: &Path, page_count: u32) -> Result<(String, u32, Vec<String>), String> {
    use windows::{
        core::HSTRING,
        Data::Pdf::PdfDocument,
        Graphics::Imaging::BitmapDecoder,
        Media::Ocr::OcrEngine,
        Storage::{StorageFile, Streams::InMemoryRandomAccessStream},
    };

    with_winrt(|| {
        let file =
            StorageFile::GetFileFromPathAsync(&HSTRING::from(path_to_string(path)))?.get()?;
        let document = PdfDocument::LoadFromFileAsync(&file)?.get()?;
        let engine = OcrEngine::TryCreateFromUserProfileLanguages()?;
        let processed_pages = page_count.min(MAX_OCR_PAGES);
        let mut pages = Vec::new();
        let mut warnings = Vec::new();
        for index in 0..processed_pages {
            let page = document.GetPage(index)?;
            let stream = InMemoryRandomAccessStream::new()?;
            page.RenderToStreamAsync(&stream)?.get()?;
            stream.Seek(0)?;
            let decoder = BitmapDecoder::CreateAsync(&stream)?.get()?;
            if let Err(err) = validate_ocr_dimensions(decoder.PixelWidth()?, decoder.PixelHeight()?)
            {
                warnings.push(format!("第 {} 页 OCR 跳过：{err}", index + 1));
                page.Close()?;
                stream.Close()?;
                continue;
            }
            let bitmap = decoder.GetSoftwareBitmapAsync()?.get()?;
            let result = engine.RecognizeAsync(&bitmap)?.get()?;
            let text = normalize_ocr_text(&result.Text()?.to_string_lossy());
            if meaningful_char_count(&text) > 0 {
                pages.push(format!("第 {} 页\n{text}", index + 1));
            } else {
                warnings.push(format!("第 {} 页未识别到文字", index + 1));
            }
            page.Close()?;
            stream.Close()?;
        }
        let joined = pages.join("\n\n");
        if meaningful_char_count(&joined) == 0 {
            return Err(windows::core::Error::new(
                windows::core::HRESULT(0x80004005_u32 as i32),
                "扫描 PDF OCR 未识别到文字",
            ));
        }
        Ok((joined, processed_pages, warnings))
    })
    .map_err(|err| format!("扫描 PDF OCR 失败：{err}"))
}

#[cfg(target_os = "windows")]
fn validate_ocr_dimensions(width: u32, height: u32) -> windows::core::Result<()> {
    use windows::Media::Ocr::OcrEngine;
    let max = OcrEngine::MaxImageDimension()?;
    if width > max || height > max {
        return Err(windows::core::Error::new(
            windows::core::HRESULT(0x80070057_u32 as i32),
            format!("图像尺寸 {width}×{height} 超过 OCR 上限 {max}"),
        ));
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn with_winrt<T>(operation: impl FnOnce() -> windows::core::Result<T>) -> windows::core::Result<T> {
    use windows::Win32::System::WinRT::{RoInitialize, RoUninitialize, RO_INIT_MULTITHREADED};
    let initialized = unsafe { RoInitialize(RO_INIT_MULTITHREADED) }.is_ok();
    let result = operation();
    if initialized {
        unsafe { RoUninitialize() };
    }
    result
}

fn normalize_ocr_text(text: &str) -> String {
    let characters = text.chars().collect::<Vec<_>>();
    let mut normalized = String::with_capacity(text.len());
    for (index, character) in characters.iter().copied().enumerate() {
        if !character.is_whitespace() {
            normalized.push(character);
            continue;
        }
        let previous = normalized.chars().next_back();
        let next = characters[index + 1..]
            .iter()
            .copied()
            .find(|value| !value.is_whitespace());
        let is_cjk_spacing = previous.zip(next).is_some_and(|(left, right)| {
            (is_cjk(left) && (is_cjk(right) || is_cjk_punctuation(right)))
                || (is_cjk_punctuation(left) && is_cjk(right))
        });
        if !is_cjk_spacing && !normalized.ends_with(' ') {
            normalized.push(' ');
        }
    }
    normalized.trim().to_string()
}

fn is_cjk(value: char) -> bool {
    matches!(
        value,
        '\u{3400}'..='\u{4DBF}'
            | '\u{4E00}'..='\u{9FFF}'
            | '\u{F900}'..='\u{FAFF}'
            | '\u{3040}'..='\u{30FF}'
            | '\u{AC00}'..='\u{D7AF}'
    )
}

fn is_cjk_punctuation(value: char) -> bool {
    matches!(
        value,
        '，' | '。' | '：' | '；' | '！' | '？' | '、' | '（' | '）' | '《' | '》' | '“' | '”'
    )
}

pub fn extracted_text_path(project_root: &Path, file_id: &str) -> PathBuf {
    project_root
        .join(".ganmaoyuan")
        .join("analysis")
        .join("content")
        .join(format!("{file_id}.txt"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::test_root;
    use std::io::Write;

    #[test]
    fn decodes_utf8_utf16_and_gbk_text() {
        let root = test_root("encoding");
        fs::create_dir_all(&root).unwrap();

        let utf8 = root.join("utf8.txt");
        fs::write(&utf8, "项目目标：整理资料").unwrap();
        assert!(parse_text_file(&utf8).unwrap().summary.contains("项目目标"));

        let utf16 = root.join("utf16.txt");
        let mut bytes = vec![0xFF, 0xFE];
        bytes.extend("项目目标：UTF16".encode_utf16().flat_map(u16::to_le_bytes));
        fs::write(&utf16, bytes).unwrap();
        assert!(parse_text_file(&utf16).unwrap().summary.contains("UTF16"));

        let gbk = root.join("gbk.txt");
        let (encoded, _, _) = GBK.encode("项目目标：GBK资料");
        fs::write(&gbk, encoded).unwrap();
        assert!(parse_text_file(&gbk).unwrap().summary.contains("GBK"));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn parses_csv_with_quotes() {
        let root = test_root("csv");
        fs::create_dir_all(&root).unwrap();
        let path = root.join("资料.csv");
        fs::write(&path, "名称,说明\n需求,\"包含,逗号\"\n").unwrap();
        let parsed = parse_delimited_file(&path, ',').unwrap();
        assert_eq!(parsed.row_count, Some(2));
        assert_eq!(parsed.column_count, Some(2));
        assert!(parsed.full_text.contains("包含,逗号"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn parses_structured_docx_xlsx_pptx_and_pdf() {
        let root = test_root("office");
        fs::create_dir_all(&root).unwrap();
        let docx = root.join("需求.docx");
        write_minimal_docx(&docx);
        let docx_result = parse_docx_file(&docx).unwrap();
        assert!(docx_result.summary.contains("段落"));
        assert!(docx_result.full_text.contains("资料名称"));

        let xlsx = root.join("数据.xlsx");
        write_minimal_xlsx(&xlsx);
        let xlsx_result = parse_workbook_file(&xlsx).unwrap();
        assert_eq!(xlsx_result.sheet_count, Some(1));
        assert!(xlsx_result.sections[0].contains("Sheet1"));

        let pptx = root.join("方案.pptx");
        write_minimal_pptx(&pptx);
        let pptx_result = parse_pptx_file(&pptx).unwrap();
        assert_eq!(pptx_result.page_count, Some(1));
        assert!(pptx_result.summary.contains("1 页"));

        let pdf = root.join("说明.pdf");
        write_minimal_pdf(&pdf);
        let pdf_result = parse_pdf_file(&pdf).unwrap();
        assert_eq!(pdf_result.page_count, Some(1));
        assert!(pdf_result.summary.contains("Ganmaoyuan"));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[ignore = "requires D-drive checkpoint A fixtures and Windows OCR"]
    fn parses_real_checkpoint_a_fixtures() {
        let root = std::env::var_os("GANMAOYUAN_ACCEPTANCE_FIXTURE_DIR")
            .map(PathBuf::from)
            .expect("GANMAOYUAN_ACCEPTANCE_FIXTURE_DIR is required");
        let root = root
            .canonicalize()
            .expect("checkpoint A fixture directory must be canonicalizable");

        assert!(parse_text_file(&root.join("01-UTF8 项目说明.txt"))
            .unwrap()
            .summary
            .contains("项目目标"));
        assert!(parse_text_file(&root.join("02-GBK 需求说明.txt"))
            .unwrap()
            .sections
            .iter()
            .any(|value| value.contains("GBK")));
        assert!(parse_text_file(&root.join("03-UTF16 决策记录.txt"))
            .unwrap()
            .sections
            .iter()
            .any(|value| value.contains("UTF-16")));
        assert_eq!(
            parse_delimited_file(&root.join("04-资料清单.csv"), ',')
                .unwrap()
                .row_count,
            Some(3)
        );
        assert!(parse_docx_file(&root.join("06-结构化需求.docx"))
            .unwrap()
            .full_text
            .contains("KEBA"));
        assert!(parse_workbook_file(&root.join("07-旧版数据.xls"))
            .unwrap()
            .sheet_count
            .is_some_and(|count| count > 0));
        assert!(parse_workbook_file(&root.join("08-复杂数据.xlsx"))
            .unwrap()
            .sheet_count
            .is_some_and(|count| count > 0));
        assert!(parse_pptx_file(&root.join("09-项目方案.pptx"))
            .unwrap()
            .full_text
            .contains("感冒院项目方案"));
        assert!(parse_pdf_file(&root.join("07-文本需求.pdf"))
            .unwrap()
            .page_count
            .is_some_and(|count| count > 0));
        let image_ocr = parse_image_file(&root.join("09-扫描需求图片.png")).unwrap();
        assert!(
            image_ocr.full_text.contains("感冒院"),
            "image OCR text: {}",
            image_ocr.full_text
        );
        let scan_pdf = parse_pdf_file(&root.join("10-扫描需求.pdf")).unwrap();
        assert!(
            scan_pdf.full_text.contains("项目目标"),
            "scan PDF OCR text: {}",
            scan_pdf.full_text
        );

        let (legacy_ppt, _) = analyze_managed_file(
            "legacy-ppt",
            "10-旧版方案.ppt",
            &root.join("10-旧版方案.ppt"),
            "ppt",
            "fixture",
        );
        assert_eq!(legacy_ppt.parse_status, "review_required");
        let (damaged_pdf, _) = analyze_managed_file(
            "damaged-pdf",
            "损坏资料.pdf",
            &root.join("损坏资料.pdf"),
            "pdf",
            "fixture",
        );
        assert_eq!(damaged_pdf.parse_status, "failed");
    }

    fn write_minimal_docx(path: &Path) {
        let file = fs::File::create(path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::FileOptions::<()>::default();
        zip.start_file("word/document.xml", options).unwrap();
        zip.write_all(r#"<?xml version="1.0" encoding="UTF-8"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>项目背景</w:t></w:r></w:p><w:tbl><w:tr><w:tc><w:p><w:r><w:t>资料名称</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>需求文档</w:t></w:r></w:p></w:tc></w:tr></w:tbl></w:body></w:document>"#.as_bytes()).unwrap();
        zip.finish().unwrap();
    }

    fn write_minimal_xlsx(path: &Path) {
        let file = fs::File::create(path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::FileOptions::<()>::default();
        zip.start_file("[Content_Types].xml", options).unwrap();
        zip.write_all(br#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/></Types>"#).unwrap();
        zip.start_file("_rels/.rels", options).unwrap();
        zip.write_all(br#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#).unwrap();
        zip.start_file("xl/_rels/workbook.xml.rels", options)
            .unwrap();
        zip.write_all(br#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#).unwrap();
        zip.start_file("xl/workbook.xml", options).unwrap();
        zip.write_all(br#"<?xml version="1.0" encoding="UTF-8"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Sheet1" sheetId="1" r:id="rId1"/></sheets></workbook>"#).unwrap();
        zip.start_file("xl/worksheets/sheet1.xml", options).unwrap();
        zip.write_all(r#"<?xml version="1.0" encoding="UTF-8"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>字段</t></is></c><c r="B1" t="inlineStr"><is><t>说明</t></is></c></row><row r="2"><c r="A2" t="inlineStr"><is><t>资料</t></is></c><c r="B2" t="inlineStr"><is><t>样例</t></is></c></row></sheetData></worksheet>"#.as_bytes()).unwrap();
        zip.finish().unwrap();
    }

    fn write_minimal_pptx(path: &Path) {
        let file = fs::File::create(path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::FileOptions::<()>::default();
        zip.start_file("ppt/slides/slide1.xml", options).unwrap();
        zip.write_all(r#"<?xml version="1.0" encoding="UTF-8"?><p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><p:cSld><a:t>项目方案</a:t><a:t>下一步</a:t></p:cSld></p:sld>"#.as_bytes()).unwrap();
        zip.finish().unwrap();
    }

    fn write_minimal_pdf(path: &Path) {
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 144] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>".to_string(),
            "<< /Length 53 >>\nstream\nBT /F1 18 Tf 40 90 Td (Ganmaoyuan PDF summary) Tj ET\nendstream".to_string(),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
        ];
        let mut pdf = String::from("%PDF-1.4\n");
        let mut offsets = Vec::new();
        for (index, object) in objects.iter().enumerate() {
            offsets.push(pdf.len());
            pdf.push_str(&format!("{} 0 obj\n{}\nendobj\n", index + 1, object));
        }
        let xref_offset = pdf.len();
        pdf.push_str("xref\n0 6\n0000000000 65535 f \n");
        for offset in offsets {
            pdf.push_str(&format!("{offset:010} 00000 n \n"));
        }
        pdf.push_str(&format!(
            "trailer << /Root 1 0 R /Size 6 >>\nstartxref\n{xref_offset}\n%%EOF"
        ));
        fs::write(path, pdf).unwrap();
    }

    #[test]
    fn business_semantics_production_order_workbook() {
        let (document_type, domain, purpose, _) = infer_business_semantics(
            "车间订单报表.xlsx",
            "订单号 | 工序 | 完成数量 | 报废数量 | 报废率\nWO-001 | 注塑 | 1200 | 12 | 1%",
            "数据表格",
        );
        assert!(
            document_type.contains("订单") || document_type.contains("生产"),
            "{document_type}"
        );
        assert_eq!(domain, "制造/生产");
        assert!(
            purpose.contains("订单") || purpose.contains("进度"),
            "{purpose}"
        );
    }

    #[test]
    fn business_semantics_vave_application() {
        let (document_type, domain, _, _) = infer_business_semantics(
            "VAVE申请单.xlsx",
            "VAVE 申请 降本 方案 提案 审批",
            "数据表格",
        );
        assert!(document_type.contains("VAVE"), "{document_type}");
        assert!(domain.contains("降本"), "{domain}");
    }

    #[test]
    fn business_semantics_weekly_plan() {
        let (document_type, _, _, _) = infer_business_semantics(
            "周工作计划.xlsx",
            "部门 | 人员 | 本周任务安排 | 完成时间",
            "数据表格",
        );
        assert!(document_type.contains("工作计划"), "{document_type}");
    }

    #[test]
    fn business_semantics_generic_table_falls_back() {
        let (document_type, domain, _, _) =
            infer_business_semantics("普通数据.xlsx", "aaa bbb ccc 123", "数据表格");
        assert_eq!(document_type, "数据表格");
        assert_eq!(domain, "未分类");
    }

    #[test]
    fn ocr_error_is_friendly_with_technical_detail() {
        let (friendly, technical) =
            friendly_parse_error("图片 OCR 失败：Windows OCR 未识别到文字 (0x80004005)");
        assert!(friendly.contains("未能从文件中识别出文字"));
        assert!(friendly.contains("重试"));
        assert!(technical.contains("0x80004005"));
    }

    #[test]
    fn non_ocr_error_keeps_original_text() {
        let (friendly, technical) =
            friendly_parse_error("旧版 .ppt 暂无可靠解析器，已进入待检查事项。");
        assert!(friendly.contains(".ppt"));
        assert!(technical.is_empty());
    }

    #[test]
    fn screenshot_with_vave_text_is_evidence_not_application() {
        let (purpose, score, evidence) = infer_document_purpose(
            "VAVE审批截图.png",
            "png",
            "VAVE 申请单 审批完成",
            &["OCR：VAVE".to_string()],
        );
        assert_eq!(purpose, "evidence");
        assert!(score >= 90);
        assert!(!evidence.is_empty());
    }

    #[test]
    fn development_status_is_report_not_plan() {
        let (purpose, score, _) = infer_document_purpose(
            "开发状态.xlsx",
            "xlsx",
            "当前状态 已完成 遗留问题 下一步",
            &["当前状态、测试结果、遗留问题".to_string()],
        );
        assert_eq!(purpose, "report");
        assert!(score >= 80);
    }
}
