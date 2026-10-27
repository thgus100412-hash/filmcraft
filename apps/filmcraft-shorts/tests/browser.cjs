const { chromium } = require("playwright");
const assert = require("node:assert/strict");
const webURL = process.env.SHORTS_WEB_URL || "http://127.0.0.1:8787";
(async () => {
  const browser = await chromium.launch({
    executablePath: "/usr/bin/chromium",
    headless: true,
    args: ["--no-sandbox"],
  });
  const context = await browser.newContext({
    viewport: { width: 1440, height: 1050 },
  });
  const page = await context.newPage();
  const errors = [];
  page.on("pageerror", (e) => errors.push(e.message));
  await page.route("**/api/health", (r) =>
    r.fulfill({
      json: {
        model_ready: false,
        speech_available: true,
        speech_models: [],
        platform: "macos",
        fonts: [],
      },
    }),
  );
  let activeFonts = ["SDCheongRyu", "GangwonEduAll-OTFBold"];
  await page.route("**/api/fonts", (r) =>
    r.fulfill({
      json: {
        native_registry: true,
        fonts: activeFonts.map((family) => ({
          family,
          styles: ["Regular"],
          source: "macos-active",
        })),
        aliases: {},
        unavailable: 0,
        warning: null,
      },
    }),
  );
  await page.route("**/api/fonts/check", (r) => {
    assert.match(r.request().headers()["content-type"], /application\/json/);
    return r.fulfill({
      json: {
        family: JSON.parse(r.request().postData()).font,
        korean: true,
        rasterized: true,
      },
    });
  });
  await page.goto(webURL);
  await page
    .getByRole("heading", { name: "긴 대화 속, 빛나는 이야기." })
    .waitFor();
  await page.screenshot({
    path: "/tmp/filmcraft-shorts-web-desktop.png",
    fullPage: true,
  });
  await page.locator("#file-input").setInputFiles([
    {
      name: "camera-wide.mp4",
      mimeType: "video/mp4",
      buffer: Buffer.from("web-upload-fixture"),
    },
    {
      name: "guest.wav",
      mimeType: "audio/wav",
      buffer: Buffer.from("web-audio-fixture"),
    },
  ]);
  await page.getByText("업로드 완료", { exact: true }).first().waitFor();
  await page.waitForFunction(
    () =>
      document.querySelectorAll(".file-state").length === 2 &&
      [...document.querySelectorAll(".file-state")].every(
        (n) => n.textContent === "업로드 완료",
      ),
  );
  assert.equal(await page.locator(".file-card").count(), 2);
  await page.getByRole("button", { name: "＋ 그룹 추가" }).click();
  assert.equal(await page.locator(".group").count(), 2);
  await page.getByLabel("camera-wide.mp4 설명").fill("게스트 클로즈업");
  await page.getByLabel("guest.wav 설명").fill("게스트 핀 마이크");
  await page.getByLabel("guest.wav 자막 화자").selectOption("crew");
  await page.getByRole("button", { name: "편집 방향 정하기" }).click();
  await page
    .locator("#brief")
    .fill("실패를 딛고 다시 시작한 경험을 담담하게 골라줘.");
  assert.equal(await page.locator("#caption-design").inputValue(), "sotam");
  await page.locator("#check-fonts").click();
  await page.getByText(/한국어 출력 검사 통과/).waitFor();
  activeFonts = ["GangwonEduAll-OTFBold"];
  await page.locator("#refresh-fonts").click();
  await page.waitForFunction(() =>
    document
      .querySelector("#font-notice")
      .textContent.includes("활성 목록에서 찾지 못한 글꼴"),
  );
  assert.equal(await page.locator("#title-font").inputValue(), "SDCheongRyu");
  activeFonts.push("SDCheongRyu");
  await page.locator("#refresh-fonts").click();
  await page.waitForFunction(() =>
    document
      .querySelector("#font-notice")
      .textContent.includes("선택한 글꼴을 찾았습니다"),
  );

  let rejectPSD = false;
  const baseStyle = {
    font: "GangwonEduAll-OTFBold",
    size: 51.8,
    color: "#fffc00",
    margin: 0.31,
    outline: 1.7,
    outline_color: "#000000",
    line_spacing: 1.2,
  };
  await page.route("**/api/caption-template", (r) =>
    r.fulfill(
      rejectPSD
        ? {
            status: 422,
            json: { error: "편집 가능한 텍스트 레이어가 필요합니다." },
          }
        : {
            json: {
              template: {
                title: {
                  ...baseStyle,
                  font: "SDCheongRyu",
                  color: "#3ef9ff",
                  size: 67.5,
                  margin: 0.07,
                },
                guest: baseStyle,
                crew: { ...baseStyle, color: "#3ef9ff" },
              },
              notice: "텍스트 스타일을 적용합니다.",
            },
          },
    ),
  );
  await page.locator("#caption-psd").setInputFiles({
    name: "captions.psd",
    mimeType: "image/vnd.adobe.photoshop",
    buffer: Buffer.from("metadata fixture"),
  });
  await page.getByText(/세?제목·제작진·게스트 스타일을 읽었습니다/).waitFor();
  assert.equal(
    await page.locator("#crew-font").inputValue(),
    "GangwonEduAll-OTFBold",
  );
  rejectPSD = true;
  await page.locator("#caption-psd").setInputFiles({
    name: "broken.psd",
    mimeType: "image/vnd.adobe.photoshop",
    buffer: Buffer.from("bad"),
  });
  await page.getByText(/PSD를 적용하지 못했습니다/).waitFor();
  assert.equal(await page.locator("#title-font").inputValue(), "SDCheongRyu");
  await page.locator("#title-font").fill("Inter");
  await page.locator("#title-font").dispatchEvent("change");
  await page.waitForTimeout(300);
  assert.equal(await page.locator("#start-job").isDisabled(), true);
  await page.screenshot({
    path: "/tmp/filmcraft-shorts-web-brief.png",
    fullPage: true,
  });
  await page.reload();
  await page.getByLabel("camera-wide.mp4 설명").waitFor();
  assert.equal(
    await page.getByLabel("guest.wav 자막 화자").inputValue(),
    "crew",
  );
  assert.equal(await page.locator("#title-font").inputValue(), "Inter");
  assert.equal(
    await page.locator("#crew-font").inputValue(),
    "GangwonEduAll-OTFBold",
  );
  assert.equal(
    await page.getByLabel("camera-wide.mp4 설명").inputValue(),
    "게스트 클로즈업",
  );
  assert.equal(
    await page.locator("#brief").inputValue(),
    "실패를 딛고 다시 시작한 경험을 담담하게 골라줘.",
  );
  await page.getByRole("button", { name: "실행 환경", exact: false }).click();
  await page.getByRole("heading", { name: "나의 로컬 스튜디오" }).waitFor();
  assert.ok(
    (await page.locator("#settings-health").innerText()).includes(
      "모델 서버 준비 필요",
    ),
  );
  await page.route("**/api/jobs", (route) =>
    route.request().method() === "GET"
      ? route.fulfill({
          json: {
            jobs: [
              {
                id: "bad-123",
                created: Date.now(),
                status: "failed",
                stage: "planning",
                can_retry: true,
                brief: "재시도 검사",
                error: "로컬 AI 응답 대기 시간을 초과했습니다.",
              },
              {
                id: "abc-123",
                created: Date.now(),
                status: "done",
                brief: "UI 결과 표시 테스트",
                title: "다시 시작할 용기",
                segments: [
                  {
                    group: 0,
                    camera: 1,
                    audio: 2,
                    reason: "<script>원문의 맥락을 유지한 발언</script>",
                  },
                ],
              },
            ],
          },
        })
      : route.continue(),
  );
  await page.getByRole("button", { name: "작업 기록", exact: false }).click();
  let retryFinished = false;
  let retryCalls = 0;
  const retried = {
    id: "def-456",
    brief: "재시도 검사",
    created: Date.now(),
    status: "running",
    stage: "planning",
    planning_status: "로컬 AI가 발언을 고르고 있습니다. 최대 대기 120분.",
    can_retry: false,
  };
  await page.route("**/api/jobs/bad-123/retry", (route) => {
    retryCalls++;
    assert.equal(route.request().method(), "POST");
    return route.fulfill({ json: retried });
  });
  await page.route("**/api/jobs/def-456", (route) =>
    route.fulfill({
      json: retryFinished
        ? { ...retried, status: "done", title: "재시도 완료", segments: [] }
        : retried,
    }),
  );
  await page.getByRole("button", { name: "상태 확인", exact: true }).click();
  await page
    .getByRole("button", { name: "분석 재사용하고 다시 시도" })
    .waitFor();
  assert.match(
    await page.locator("#progress-description").textContent(),
    /중단/,
  );
  await page.screenshot({
    path: "/tmp/shorts-planner-retry.png",
    fullPage: true,
  });
  await page.setViewportSize({ width: 390, height: 844 });
  await page.screenshot({
    path: "/tmp/shorts-planner-retry-mobile.png",
    fullPage: true,
  });
  await page.setViewportSize({ width: 1440, height: 1050 });
  await page.getByRole("button", { name: "분석 재사용하고 다시 시도" }).click();
  await page.waitForFunction(() =>
    document
      .querySelector("#progress-description")
      .textContent.includes("120분"),
  );
  assert.equal(retryCalls, 1);
  retryFinished = true;
  await page
    .getByRole("heading", { name: "재시도 완료", exact: true })
    .waitFor();
  await page.getByRole("button", { name: "작업 기록", exact: false }).click();
  await page.getByRole("button", { name: "결과 열기" }).click();
  assert.equal(
    await page.locator("#result-title").innerText(),
    "다시 시작할 용기",
  );
  assert.equal(await page.locator("#result-actions a").count(), 4);
  assert.ok(
    (await page.locator("#selections").innerText()).includes("<script>"),
  );
  assert.equal(await page.locator("#selections script").count(), 0);
  await page
    .getByRole("button", { name: "새 쇼츠 만들기 ＋", exact: true })
    .click();
  await page.setViewportSize({ width: 390, height: 844 });
  await page.screenshot({
    path: "/tmp/filmcraft-shorts-web-mobile.png",
    fullPage: true,
  });
  assert.equal(
    await page.evaluate(
      () => document.documentElement.scrollWidth > window.innerWidth,
    ),
    false,
  );
  assert.deepEqual(errors, []);
  console.log(
    "Browser checks passed: upload, roles, grouping, brief, readiness, draft restore, history, result downloads, safe text, mobile layout.",
  );
  await page.getByRole("button", { name: /실행 환경/ }).click();
  await page
    .getByText("처음 사용하는 경우: 모델 준비 안내", { exact: true })
    .click();
  assert.equal(await page.locator("#mac-model-help").isVisible(), true);
  assert.equal(await page.locator("#linux-model-help").isVisible(), false);
  assert.match(
    await page.locator("#mac-model-help").textContent(),
    /setup-mac-ai.sh/,
  );
  await page.screenshot({
    path: "/tmp/filmcraft-shorts-mac-settings.png",
    fullPage: true,
  });
  await browser.close();
})().catch((e) => {
  console.error(e);
  process.exit(1);
});
