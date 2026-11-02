use super::*;

fn texts(s: &Scene) -> Vec<String> {
    s.items.iter().filter_map(|i| if let Item::Text { text, .. } = i { Some(text.clone()) } else { None }).collect()
}

#[test]
fn 流程图基本节点与连线() {
    let s = render("graph TD\n  A[开始] --> B{判断}\n  B -->|是| C(处理)\n  B -->|否| D([结束])\n  C --> D").unwrap();
    let t = texts(&s);
    for w in ["开始", "判断", "是", "否", "处理", "结束"] {
        assert!(t.contains(&w.to_string()), "缺少 {w}: {t:?}");
    }
    assert!(s.width > 100. && s.height > 100.);
}

#[test]
fn 方向影响宽高比() {
    let td = render("graph TD\n A-->B-->C-->D").unwrap();
    let lr = render("graph LR\n A-->B-->C-->D").unwrap();
    assert!(td.height > td.width);
    assert!(lr.width > lr.height);
}

#[test]
fn 链式与并列和环() {
    let s = render("flowchart LR\n A & B --> C --> D\n D --> A\n C --> C").unwrap();
    assert!(texts(&s).contains(&"D".to_string()));
}

#[test]
fn 各种连线写法() {
    for src in ["A --- B", "A -.-> B", "A ==> B", "A --o B", "A --x B", "A <--> B", "A -- 文字 --> B", "A -. 虚 .-> B", "A == 粗 ==> B", "A-->|x|B"] {
        render(&format!("graph LR\n {src}")).unwrap_or_else(|e| panic!("{src}: {e}"));
    }
}

#[test]
fn 子图与样式() {
    let src = "graph TB\n subgraph one [第一组]\n  A --> B\n end\n subgraph two\n  C --> D\n end\n B --> C\n style A fill:#f9f,stroke:#333\n classDef hot fill:#f00,color:#fff\n class D hot\n E:::hot --> A";
    let s = render(src).unwrap();
    assert!(texts(&s).contains(&"第一组".to_string()));
}

#[test]
fn 所有节点形状() {
    let src = "graph LR\n A[矩形] --> B(圆角) --> C([体育场]) --> D[[子程序]] --> E[(数据库)] --> F((圆)) --> G{菱形} --> H{{六边形}} --> I[/平行/] --> J[\\反平行\\] --> K[/梯形\\] --> L[\\倒梯形/] --> M>旗帜]";
    let s = render(src).unwrap();
    assert_eq!(texts(&s).len(), 13);
}

#[test]
fn 带引号和换行的标签() {
    let s = render("graph TD\n A[\"带 (括号) 的\\n两行\"] --> B[a<br/>b]").unwrap();
    let t = texts(&s);
    assert!(t.contains(&"带 (括号) 的".to_string()) && t.contains(&"两行".to_string()), "{t:?}");
}

#[test]
fn 流程图语法错误() {
    assert!(matches!(render("graph TD\n A --> "), Err(RenderError::Syntax(_))));
    assert!(matches!(render("graph TD\n ???"), Err(RenderError::Syntax(_))));
}

#[test]
fn 时序图() {
    let src = "sequenceDiagram\n title 登录\n autonumber\n participant U as 用户\n actor A as 管理员\n U->>S: 请求\n activate S\n S-->>U: 响应\n deactivate S\n S->>S: 自检\n Note over U,S: 握手\n Note right of S: 备注\n loop 每分钟\n  U-)S: 心跳\n else 失败\n  S--xU: 断开\n end\n alt 成功\n  U->>+S: a\n  S-->>-U: b\n end";
    let s = render(src).unwrap();
    let t = texts(&s);
    assert!(t.iter().any(|x| x.contains("请求")) && t.contains(&"用户".to_string()) && t.contains(&"登录".to_string()), "{t:?}");
}

#[test]
fn 饼图() {
    let s = render("pie showData title 耗时\n \"渲染\" : 30\n \"网络\" : 50\n \"其他\" : 20").unwrap();
    let t = texts(&s);
    assert!(t.contains(&"耗时".to_string()) && t.contains(&"50%".to_string()) && t.iter().any(|x| x.starts_with("网络")), "{t:?}");
    assert!(matches!(render("pie\n \"a\" : x"), Err(RenderError::Syntax(_))));
}

#[test]
fn 状态图() {
    let src = "stateDiagram-v2\n [*] --> Idle\n Idle --> Running : 启动\n Running --> Idle : 停止\n Running --> [*]\n state \"忙碌中\" as Busy\n Running --> Busy\n note right of Idle\n  说明文字\n end note";
    let s = render(src).unwrap();
    assert!(texts(&s).contains(&"启动".to_string()));
}

#[test]
fn 不支持的类型与空内容() {
    assert!(matches!(render("gantt\n title x"), Err(RenderError::Unsupported(k)) if k == "gantt"));
    assert!(matches!(render("   \n"), Err(RenderError::Empty)));
    assert_eq!(kind_title("graph TD\nA-->B"), Some("流程图"));
    assert_eq!(kind_title("classDiagram"), None);
}

#[test]
fn 注释与前置元数据被跳过() {
    render("---\ntitle: x\n---\n%% 注释\ngraph TD\n  A-->B").unwrap();
}

#[test]
fn 大图性能与上限() {
    let mut src = String::from("graph TD\n");
    for i in 0..120 {
        src.push_str(&format!(" N{i} --> N{}\n N{i} --> N{}\n", i + 1, i + 2));
    }
    let t = std::time::Instant::now();
    render(&src).unwrap();
    assert!(t.elapsed().as_secs() < 5);
    let mut huge = String::from("graph TD\n");
    for i in 0..400 {
        huge.push_str(&format!(" X{i} --> Y{i}\n"));
    }
    assert!(matches!(render(&huge), Err(RenderError::TooLarge(_))));
}
