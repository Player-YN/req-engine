//! Seat-specific onboarding prompts copied with a pair code. Zero LLM.
//! Portable: no issuer machine paths, no product brochure, duties only.

use crate::domain::models::AgentSeat;

/// Windows install location when shipped as a setup (same on every PC, env-expanded).
pub const INSTALL_EXE_WIN: &str = r"%LOCALAPPDATA%\Programs\req-engine\req-engine.exe";
/// Default data home (same formula on every PC).
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
        r#"# 如何连接 MCP Server（必做，否则没有工具）
这是 **stdio MCP**：你的 coding agent 必须拉起一个本地子进程，在 stdin/stdout 上讲 MCP。
没有网址，不要连 HTTP / WebSocket。

1. 找到可执行文件（按顺序，不要用别人机器上的盘符）：
   - PATH 里的 `req-engine` 或 `req-engine.exe`
   - 否则 `{inst}`（安装包约定；Windows 会展开成当前用户的 LocalAppData）
2. 数据目录 `--home`：
   - 环境变量 `REQ_ENGINE_HOME`（若已设置）
   - 否则 `{home}`（展开为当前用户主目录下的 .req-engine）
3. 在你所用软件的 MCP / 本地 MCP server / stdio server 设置里新增一条（名字随意）：

   command = <步骤 1 的完整路径或 req-engine.exe>
   args    = ["mcp", "--pair", "{code}", "--home", "<步骤 2 的目录>"]

   配置文件叫什么、菜单在哪，查你所用软件自己的文档。契约只有：spawn 上面这个进程，stdio 讲 MCP。
   若该软件不能当 MCP client，停下来告诉人，不要伪造连接。

4. 配好后应能看到本座位的工具列表。initialize 报 invalid pair：home 和码不是同一份数据，问人，不要改码。
不要用 tokens.txt，不要加 --role / --token。stdout 只走协议，日志在 stderr。匹配码不要写进 git。"#,
        code = code,
        home = DEFAULT_HOME_WIN,
        inst = INSTALL_EXE_WIN,
    )
}

pub fn discuss_onboarding_prompt(ctx: &OnboardingCtx<'_>) -> String {
    format!(
        r#"# 职责
跟用户把项目需求讨论清楚，写成看板上的卡片。不要自己下场改代码、实现功能。

第一次接到这个项目：先通读代码工作区（宿主当前打开的文件夹），弄清现在做到哪，再讨论、再建卡。

# 匹配码
{code}

{howto}

# 工具
- list_requirements / get_requirement
- create_requirement（一定是 todo）
- update_requirement、cancel_requirement（仅限仍是 todo）

# 不要做
- 不要改业务代码、不要开实现分支、不要 claim
- 不要动 in_progress / review 的卡
- 不要把卡标成完成
"#,
        code = ctx.pair_code,
        howto = mcp_connect(ctx.pair_code),
    )
}

pub fn build_onboarding_prompt(ctx: &OnboardingCtx<'_>) -> String {
    format!(
        r#"# 职责
先通读代码工作区（宿主当前打开的文件夹），弄清进度。然后只按看板里就绪的 todo 卡片做事，不要做卡片以外的越界改动。

# 匹配码
{code}

{howto}

# 工具
- list_ready_tasks / get_requirement
- claim_task（可连领多张，自己排并行或串行）
- report_progress / submit_for_review / release_task

# 不要做
- 不要自己建需求、改需求文案
- 不要做 todo 卡片范围以外的事
- 不要 complete_review（人在桌面审）
"#,
        code = ctx.pair_code,
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

    fn assert_portable_pack(t: &str) {
        assert!(t.contains("stdio"));
        assert!(t.contains("如何连接 MCP"));
        assert!(t.contains("command ="));
        assert!(t.contains("args    ="));
        assert!(t.contains("REQ_ENGINE_HOME"));
        assert!(t.contains(DEFAULT_HOME_WIN));
        assert!(t.contains(INSTALL_EXE_WIN));
        assert!(!t.contains("你是谁"));
        assert!(!t.contains("当前状况"));
        assert!(!t.contains("产品：需求引擎"));
        assert!(!t.to_ascii_lowercase().contains("grok"));
        assert!(!t.to_ascii_lowercase().contains("cursor"));
        assert!(!t.contains("127.0.0.1"));
        assert!(!t.contains(r"C:\Users\yyy\secret-box"));
        assert!(!t.contains(r"C:\Users\yyy\secret-home"));
        assert!(!t.contains(r"C:\Users\yyy\some-repo"));
    }

    #[test]
    fn discuss_prompt_has_boundaries_and_pair() {
        let t = discuss_onboarding_prompt(&ctx());
        assert!(t.contains("disc_abc"));
        assert!(t.contains("--pair"));
        assert!(t.contains("不要自己下场"));
        assert!(t.contains("通读代码工作区"));
        assert!(t.contains("list_requirements"));
        assert!(!t.contains("claim_task"));
        assert_portable_pack(&t);
    }

    #[test]
    fn build_prompt_forbids_complete_review() {
        let mut c = ctx();
        c.pair_code = "build_xyz";
        let t = build_onboarding_prompt(&c);
        assert!(t.contains("build_xyz"));
        assert!(t.contains("claim_task"));
        assert!(t.contains("不要 complete_review"));
        assert!(t.contains("不要做卡片以外的越界") || t.contains("不要做 todo 卡片范围以外"));
        assert!(t.contains("通读代码工作区"));
        assert!(!t.contains("create_requirement"));
        assert_portable_pack(&t);
    }
}
