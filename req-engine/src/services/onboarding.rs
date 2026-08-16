//! Seat-specific onboarding prompts copied with a pair code. Zero LLM.
//! Agent-facing connect recipe only: portable command/args, duty, tools on this seat.

use crate::domain::models::AgentSeat;

pub const INSTALL_EXE_WIN: &str = r"%LOCALAPPDATA%\Programs\req-engine\req-engine.exe";
pub const DEFAULT_HOME_WIN: &str = r"%USERPROFILE%\.req-engine";

pub struct OnboardingCtx<'a> {
    pub exe: &'a str,
    pub home: &'a str,
    pub project_id: &'a str,
    pub project_name: &'a str,
    pub pair_code: &'a str,
    pub local_path: &'a str,
}

pub fn onboarding_prompt(seat: AgentSeat, ctx: &OnboardingCtx<'_>) -> String {
    match seat {
        AgentSeat::Discuss => discuss_onboarding_prompt(ctx),
        AgentSeat::Build => build_onboarding_prompt(ctx),
    }
}

fn mcp_connect(code: &str) -> String {
    format!(
        r#"# 连接 MCP
stdio。登记一条本地 MCP server：

  command = req-engine.exe
  args    = ["mcp", "--pair", "{code}"]

若 PATH 里没有 req-engine.exe，command 用：
  {inst}

数据目录：`REQ_ENGINE_HOME`，否则 `{home}`。"#,
        code = code,
        inst = INSTALL_EXE_WIN,
        home = DEFAULT_HOME_WIN,
    )
}

pub fn discuss_onboarding_prompt(ctx: &OnboardingCtx<'_>) -> String {
    format!(
        r#"# 职责
跟用户把项目需求讨论清楚，写成看板上的卡片。不要自己下场改代码或实现功能。

第一次接到这个项目：先通读代码工作区（宿主当前打开的文件夹），弄清现在做到哪，再讨论、再建卡。

{howto}

# 工具
- list_requirements
- get_requirement
- create_requirement
- update_requirement
- cancel_requirement
"#,
        howto = mcp_connect(ctx.pair_code),
    )
}

pub fn build_onboarding_prompt(ctx: &OnboardingCtx<'_>) -> String {
    format!(
        r#"# 职责
先通读代码工作区（宿主当前打开的文件夹），弄清进度。只按看板里就绪的 todo 卡片完成任务，不要做卡片以外的改动。

{howto}

# 工具
- list_ready_tasks
- get_requirement
- claim_task
- report_progress
- submit_for_review
- release_task
"#,
        howto = mcp_connect(ctx.pair_code),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> OnboardingCtx<'static> {
        OnboardingCtx {
            exe: r"C:\Users\yyy\secret-box\req-engine.exe",
            home: r"C:\Users\yyy\secret-home",
            project_id: "proj-1",
            project_name: "Demo",
            pair_code: "disc_abc",
            local_path: r"C:\Users\yyy\some-repo",
        }
    }

    fn assert_one_click_pack(t: &str) {
        assert!(t.contains("stdio"));
        assert!(t.contains("command = req-engine.exe"));
        assert!(t.contains(r#"["mcp", "--pair""#));
        assert!(t.contains(INSTALL_EXE_WIN));
        assert!(t.contains(DEFAULT_HOME_WIN));
        assert!(!t.contains("--home"));
        assert!(!t.contains("你是谁"));
        assert!(!t.contains("不要伪造"));
        assert!(!t.contains("问使用你的人"));
        assert!(!t.contains("不要抄别人"));
        assert!(!t.contains("tokens.txt"));
        assert!(!t.to_ascii_lowercase().contains("grok"));
        assert!(!t.to_ascii_lowercase().contains("cursor"));
        assert!(!t.contains(r"C:\Users\yyy\secret-box"));
        assert!(!t.contains(r"C:\Users\yyy\secret-home"));
        assert!(!t.contains(r"C:\Users\yyy\some-repo"));
    }

    #[test]
    fn discuss_prompt_has_boundaries_and_pair() {
        let t = discuss_onboarding_prompt(&ctx());
        assert!(t.contains("disc_abc"));
        assert!(t.contains("不要自己下场"));
        assert!(t.contains("通读代码工作区"));
        assert!(t.contains("list_requirements"));
        assert!(!t.contains("claim"));
        assert!(!t.contains("complete_review"));
        assert_one_click_pack(&t);
    }

    #[test]
    fn build_prompt_has_duty_and_pair() {
        let mut c = ctx();
        c.pair_code = "build_xyz";
        let t = build_onboarding_prompt(&c);
        assert!(t.contains("build_xyz"));
        assert!(t.contains("claim_task"));
        assert!(t.contains("只按看板里就绪的 todo"));
        assert!(!t.contains("complete_review"));
        assert!(!t.contains("create_requirement"));
        assert_one_click_pack(&t);
    }
}
