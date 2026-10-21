"use strict";
const $ = (s) => document.querySelector(s);
const $$ = (s) => [...document.querySelectorAll(s)];
const state = {
  groups: [{ name: "인터뷰 촬영 01", sync: "audio" }],
  files: [],
  step: 1,
  health: null,
  job: null,
  jobs: [],
  polling: false,
};
let nextFile = 0;
function el(tag, className, text) {
  const n = document.createElement(tag);
  if (className) n.className = className;
  if (text !== undefined) n.textContent = text;
  return n;
}
function notify(message) {
  $("#notice").textContent = message;
  $("#notice").classList.remove("hidden");
}
function clearNotice() {
  $("#notice").classList.add("hidden");
}
function toast(message) {
  $("#toast").textContent = message;
  $("#toast").classList.remove("hidden");
  setTimeout(() => $("#toast").classList.add("hidden"), 3500);
}
async function api(path, options = {}) {
  const response = await fetch(path, options);
  let value;
  try {
    value = await response.json();
  } catch {
    throw new Error(
      "서버 응답을 확인할 수 없습니다. 서버가 켜져 있는지 확인해 주세요.",
    );
  }
  if (!response.ok)
    throw new Error(value.error || "요청을 처리할 수 없습니다.");
  return value;
}
function saveDraft() {
  try {
    localStorage.setItem(
      "filmcraft-shorts-draft",
      JSON.stringify({
        groups: state.groups,
        files: state.files
          .filter((f) => f.id)
          .map(({ id, name, size, role, label, group }) => ({
            id,
            name,
            size,
            role,
            label,
            group,
          })),
        brief: $("#brief").value,
        duration: $("#duration").value,
        speech: $("#speech-model").value,
      }),
    );
  } catch {
    /* Browser storage may be disabled; server files remain intact. */
  }
}
function restoreDraft() {
  try {
    const d = JSON.parse(
      localStorage.getItem("filmcraft-shorts-draft") || "null",
    );
    if (
      d &&
      Array.isArray(d.groups) &&
      d.groups.length &&
      d.groups.length <= 32 &&
      Array.isArray(d.files) &&
      d.files.length <= 128
    ) {
      state.groups = d.groups.map((g) => ({
        name: String(g.name).slice(0, 200),
        sync: g.sync === "timecode" ? "timecode" : "audio",
      }));
      state.files = d.files
        .filter(
          (f) =>
            typeof f.id === "string" &&
            /^[0-9a-f-]{1,64}$/.test(f.id) &&
            Number.isInteger(f.group) &&
            f.group >= 0 &&
            f.group < state.groups.length,
        )
        .map((f) => ({
          ...f,
          key: `f${++nextFile}`,
          role: f.role === "recordings" ? "recordings" : "cameras",
          label: String(f.label || "").slice(0, 500),
          status: "ready",
          progress: 100,
        }));
      $("#brief").value = String(d.brief || "").slice(0, 4000);
      if (["30", "45", "60", "90", "180"].includes(d.duration))
        $("#duration").value = d.duration;
      if (["whisper-tiny", "whisper-base", "whisper-small"].includes(d.speech))
        $("#speech-model").value = d.speech;
    }
  } catch {
    /* Start with an empty draft if local data is invalid. */
  }
}
function page(name) {
  $$(".page").forEach((n) =>
    n.classList.toggle("hidden", n.id !== `${name}-page`),
  );
  $$(".nav").forEach((n) =>
    n.classList.toggle("active", n.dataset.page === name),
  );
  $("#breadcrumb").textContent =
    `워크스페이스 / ${{ create: "새 쇼츠 만들기", history: "작업 기록", settings: "실행 환경" }[name]}`;
  clearNotice();
  if (name === "history") loadHistory();
  if (name === "settings") checkHealth();
}
function showStep(step) {
  state.step = step;
  $$("[data-stage]").forEach((n) =>
    n.classList.toggle("hidden", Number(n.dataset.stage) !== step),
  );
  $$("[data-step]").forEach((n) => {
    const s = Number(n.dataset.step);
    n.classList.toggle("active", s === step);
    n.classList.toggle("done", s < step);
    n.setAttribute("aria-current", s === step ? "step" : "false");
  });
  clearNotice();
}
function sizeLabel(size) {
  return size >= 1024 ** 3
    ? `${(size / 1024 ** 3).toFixed(2)} GB`
    : `${(size / 1024 ** 2).toFixed(1)} MB`;
}
function select(options, value, handler, description) {
  const s = el("select");
  s.setAttribute("aria-label", description);
  for (const [id, text] of options) {
    const o = el("option", "", text);
    o.value = id;
    s.append(o);
  }
  s.value = String(value);
  s.addEventListener("change", () => handler(s.value));
  return s;
}
function renderGroups() {
  const root = $("#groups");
  root.replaceChildren();
  $("#file-summary").textContent = `파일 ${state.files.length}개`;
  state.groups.forEach((group, gi) => {
    const box = el("section", "group");
    const head = el("div", "group-head");
    head.append(
      el("span", "group-number", `GROUP ${String(gi + 1).padStart(2, "0")}`),
    );
    const name = el("input", "group-name");
    name.value = group.name;
    name.maxLength = 60;
    name.setAttribute("aria-label", `촬영 그룹 ${gi + 1} 이름`);
    name.addEventListener("input", () => {
      group.name = name.value;
      saveDraft();
    });
    head.append(name);
    const sync = select(
      [
        ["audio", "오디오로 싱크"],
        ["timecode", "공유 타임코드"],
      ],
      group.sync,
      (v) => {
        group.sync = v;
        saveDraft();
      },
      `그룹 ${gi + 1} 동기화 방식`,
    );
    sync.className = "group-sync";
    head.append(sync);
    box.append(head);
    const files = state.files.filter((f) => f.group === gi);
    if (!files.length)
      box.append(
        el(
          "div",
          "group-empty",
          "파일을 추가하거나 다른 그룹의 파일을 이곳으로 이동해 주세요.",
        ),
      );
    files.forEach((f) => {
      const card = el("div", "file-card");
      card.dataset.key = f.key;
      const top = el("div", "file-top");
      top.append(
        el(
          "span",
          `file-kind ${f.role === "recordings" ? "audio" : ""}`,
          f.role === "recordings" ? "AUDIO" : "VIDEO",
        ),
      );
      const meta = el("div", "file-meta");
      meta.append(
        el("div", "file-name", f.name),
        el("div", "file-size", sizeLabel(f.size)),
      );
      const status = el(
        "span",
        `file-state ${f.status === "error" ? "error" : ""}`,
        f.status === "ready"
          ? "업로드 완료"
          : f.status === "error"
            ? "업로드 실패"
            : `${f.progress || 0}%`,
      );
      status.title = f.error || "";
      const remove = el("button", "file-remove", "×");
      remove.setAttribute("aria-label", `${f.name} 목록에서 제거`);
      remove.addEventListener("click", async () => {
        remove.disabled = true;
        try {
          if (f.xhr) f.xhr.abort();
          if (f.id) {
            const data = await api(`/api/files/${f.id}`, { method: "DELETE" });
            if (data.retained)
              toast("작업에서 사용한 파일은 보존하고 목록에서만 제거했어요.");
          }
          state.files = state.files.filter((x) => x !== f);
          saveDraft();
          renderGroups();
        } catch (e) {
          notify(e.message);
          remove.disabled = false;
        }
      });
      top.append(meta, status, remove);
      card.append(top);
      const options = el("div", "file-options");
      options.append(
        select(
          [
            ["cameras", "카메라 영상"],
            ["recordings", "별도 녹음"],
          ],
          f.role,
          (v) => {
            f.role = v;
            saveDraft();
            renderGroups();
          },
          `${f.name} 역할`,
        ),
        select(
          state.groups.map((g, i) => [String(i), `그룹 ${i + 1}`]),
          String(f.group),
          (v) => {
            f.group = Number(v);
            saveDraft();
            renderGroups();
          },
          `${f.name} 촬영 그룹`,
        ),
      );
      const label = el("input");
      label.value = f.label;
      label.placeholder =
        f.role === "recordings"
          ? "예: 게스트 핀 마이크"
          : "예: 게스트 클로즈업";
      label.maxLength = 150;
      label.setAttribute("aria-label", `${f.name} 설명`);
      label.addEventListener("input", () => {
        f.label = label.value;
        saveDraft();
      });
      options.append(label);
      card.append(options);
      if (f.status !== "ready") {
        const track = el("div", "upload-track");
        const fill = el("div", "upload-fill");
        fill.style.width = `${f.progress || 0}%`;
        track.append(fill);
        card.append(track);
      }
      box.append(card);
    });
    root.append(box);
  });
}
function uploadFile(item, file) {
  return new Promise((resolve) => {
    const xhr = new XMLHttpRequest();
    item.xhr = xhr;
    xhr.open("POST", "/api/files");
    xhr.setRequestHeader("X-File-Name", encodeURIComponent(file.name));
    xhr.setRequestHeader("Content-Type", "application/octet-stream");
    xhr.upload.addEventListener("progress", (event) => {
      if (event.lengthComputable) {
        item.progress = Math.floor((event.loaded / event.total) * 100);
        const row = $(`[data-key="${item.key}"]`);
        if (row) {
          const label = row.querySelector(".file-state");
          label.textContent =
            item.progress === 100 ? "저장 중" : `${item.progress}%`;
          const fill = row.querySelector(".upload-fill");
          if (fill) fill.style.width = `${item.progress}%`;
        }
      }
    });
    xhr.onload = () => {
      try {
        const data = JSON.parse(xhr.responseText);
        if (xhr.status < 200 || xhr.status >= 300)
          throw new Error(data.error || "업로드 실패");
        item.id = data.id;
        item.status = "ready";
        item.progress = 100;
      } catch (e) {
        item.status = "error";
        item.error = e.message;
        notify(`${file.name}: ${e.message}`);
      }
      item.xhr = null;
      saveDraft();
      renderGroups();
      resolve();
    };
    xhr.onerror = () => {
      item.status = "error";
      item.error = "서버 연결이 끊겼습니다.";
      item.xhr = null;
      renderGroups();
      resolve();
    };
    xhr.onabort = () => {
      item.status = "error";
      item.xhr = null;
      resolve();
    };
    xhr.send(file);
  });
}
let uploadQueue = Promise.resolve();
function addFiles(files) {
  clearNotice();
  for (const file of files) {
    if (state.files.length >= 128) {
      notify("최대 128개 파일을 추가할 수 있습니다.");
      break;
    }
    if (!file.size || file.size > 8 * 1024 ** 3) {
      notify(`${file.name}: 빈 파일이거나 8GB를 넘습니다.`);
      continue;
    }
    const role = /\.(wav|flac|mp3|m4a|aac|ogg|opus|aiff)$/i.test(file.name)
      ? "recordings"
      : "cameras";
    const item = {
      key: `f${++nextFile}`,
      name: file.name,
      size: file.size,
      role,
      label: "",
      group: 0,
      status: "uploading",
      progress: 0,
    };
    state.files.push(item);
    uploadQueue = uploadQueue.then(() =>
      state.files.includes(item) ? uploadFile(item, file) : undefined,
    );
  }
  renderGroups();
}
function mediaProblem() {
  if (!state.files.length) return "먼저 촬영 영상과 녹음을 추가해 주세요.";
  if (state.files.some((f) => f.status !== "ready"))
    return "모든 파일의 업로드가 완료되어야 합니다. 실패한 파일은 제거 후 다시 추가해 주세요.";
  for (let i = 0; i < state.groups.length; i++) {
    const files = state.files.filter((f) => f.group === i);
    if (files.length && !files.some((f) => f.role === "cameras"))
      return `그룹 ${i + 1}에 카메라 영상이 필요합니다.`;
    if (files.length > 32)
      return `그룹 ${i + 1}에는 최대 32개 파일을 넣을 수 있습니다.`;
  }
  return null;
}
async function checkHealth() {
  try {
    state.health = await api("/api/health");
    const h = state.health;
    const ready = h.model_ready && h.speech_available;
    $("#connection").textContent = ready
      ? "● 로컬 스튜디오 준비됨"
      : "○ 로컬 AI 준비 필요";
    $("#connection").classList.toggle("ready", ready);
    $("#readiness").textContent = ready
      ? "로컬 AI가 준비됐어요. 편집을 시작할 수 있습니다."
      : "아직 편집을 시작할 수 없어요. 실행 환경에서 로컬 AI 준비 안내를 확인해 주세요.";
    $("#readiness").classList.toggle("blocked", !ready);
    $("#start-job").disabled =
      !ready || state.jobs.some((j) => j.status === "running");
    const list = $("#settings-health");
    list.replaceChildren();
    for (const [label, value] of [
      ["로컬 편집 AI", h.model_ready ? "준비 완료" : "모델 서버 준비 필요"],
      [
        "한국어 음성 인식 기능",
        h.speech_available ? "사용 가능" : "Whisper 포함 빌드 필요",
      ],
      ["대본 처리 위치", "이 컴퓨터 안에서만 처리"],
    ]) {
      const row = el("div", "settings-row");
      row.append(el("span", "", label), el("span", "", value));
      list.append(row);
    }
    for (const m of h.speech_models || []) {
      const row = el("div", "settings-row");
      row.append(
        el("span", "", m.id),
        el("span", "", m.installed ? "설치됨" : "첫 사용 시 다운로드 필요"),
      );
      list.append(row);
    }
  } catch (e) {
    state.health = null;
    $("#connection").textContent = "서버 연결 확인 필요";
    $("#start-job").disabled = true;
    $("#readiness").textContent = e.message;
  }
}
async function startJob() {
  clearNotice();
  const problem = mediaProblem();
  if (problem) {
    showStep(1);
    notify(problem);
    return;
  }
  const brief = $("#brief").value.trim();
  if (!brief) {
    notify("원하는 주제와 뉘앙스를 입력해 주세요.");
    $("#brief").focus();
    return;
  }
  $("#start-job").disabled = true;
  saveDraft();
  const groups = state.groups
    .map((g, i) => ({
      name: g.name || `촬영 ${i + 1}`,
      sync: g.sync,
      cameras: state.files
        .filter((f) => f.group === i && f.role === "cameras")
        .map((f) => ({ file_id: f.id, label: f.label })),
      recordings: state.files
        .filter((f) => f.group === i && f.role === "recordings")
        .map((f) => ({ file_id: f.id, label: f.label })),
    }))
    .filter((g) => g.cameras.length || g.recordings.length);
  try {
    const job = await api("/api/jobs", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        brief,
        max_seconds: Number($("#duration").value),
        speech_model: $("#speech-model").value,
        groups,
      }),
    });
    state.job = job;
    $("#result-video").pause();
    $("#result-video").classList.add("hidden");
    $("#preview-empty").classList.remove("hidden");
    showStep(3);
    renderProgress(job);
    pollJob();
    loadHistory();
  } catch (e) {
    notify(e.message);
    checkHealth();
  }
}
const stages = [
  [
    "analysis",
    "동기화와 한국어 대본 분석",
    "촬영 그룹을 맞추고 단어별 대본을 만들어요.",
  ],
  [
    "planning",
    "주제에 맞는 발언 선택",
    "로컬 AI가 맥락과 뉘앙스를 살펴 제목과 편집 계획을 정해요.",
  ],
  [
    "assembling",
    "카메라·마이크와 자막 구성",
    "선택한 구간을 연결하고 원문 자막을 배치해요.",
  ],
  ["rendering", "세로 영상 출력", "제목과 자막이 들어간 MP4를 만들어요."],
];
function renderProgress(job) {
  const stopped = ["failed", "cancelled", "interrupted"].includes(job.status);
  $("#progress-title").textContent = stopped
    ? "편집이 멈췄어요"
    : "이야기를 엮고 있어요";
  $("#live-pill").textContent = stopped
    ? { failed: "FAILED", cancelled: "CANCELLED", interrupted: "INTERRUPTED" }[
        job.status
      ]
    : "PROCESSING";
  $("#cancel-job").classList.toggle("hidden", job.status !== "running");
  const error = $("#job-error");
  error.classList.toggle("hidden", !stopped);
  error.textContent =
    job.error ||
    (job.status === "cancelled"
      ? "작업을 취소했습니다. 원본 파일은 그대로 보관됩니다."
      : "");
  const stage = Math.max(
    0,
    stages.findIndex((s) => s[0] === job.stage),
  );
  const root = $("#progress-stages");
  root.replaceChildren();
  stages.forEach(([key, title, description], i) => {
    const row = el(
      "div",
      `progress-item ${i < stage ? "done" : i === stage && !stopped ? "active" : ""}`,
    );
    row.append(
      el(
        "span",
        "stage-marker",
        i < stage ? "✓" : String(i + 1).padStart(2, "0"),
      ),
    );
    const text = el("div");
    text.append(el("strong", "", title), el("p", "", description));
    row.append(text);
    root.append(row);
  });
}
async function pollJob() {
  if (state.polling) return;
  state.polling = true;
  try {
    while (state.job?.status === "running") {
      await new Promise((r) => setTimeout(r, 1500));
      const id = state.job.id;
      try {
        const job = await api(`/api/jobs/${id}`);
        if (state.job?.id !== id) break;
        state.job = job;
        renderProgress(job);
        if (job.status === "done") {
          showResult(job);
          loadHistory();
          break;
        }
        if (job.status !== "running") {
          loadHistory();
          checkHealth();
        }
      } catch (e) {
        notify(e.message);
        break;
      }
    }
  } finally {
    state.polling = false;
    if (state.job?.status === "running") setTimeout(pollJob, 2000);
  }
}
function showResult(job) {
  state.job = job;
  page("create");
  showStep(4);
  $("#result-title").textContent = job.title || "완성된 쇼츠";
  const video = $("#result-video");
  video.src = `/api/jobs/${job.id}/files/short.mp4`;
  video.classList.remove("hidden");
  $("#preview-empty").classList.add("hidden");
  const actions = $("#result-actions");
  actions.replaceChildren();
  for (const [name, label, primary] of [
    ["short.mp4", "MP4 다운로드 ↓", true],
    ["captions.srt", "자막 SRT", false],
    ["short.fcproj", "편집 프로젝트", false],
    ["plan.json", "편집 계획", false],
  ]) {
    const a = el("a", `button ${primary ? "primary" : "secondary"}`, label);
    a.href = `/api/jobs/${job.id}/files/${name}`;
    a.download = name;
    actions.append(a);
  }
  const selections = $("#selections");
  selections.replaceChildren();
  (job.segments || []).forEach((s, i) => {
    const card = el("div", "selection");
    card.append(
      el(
        "strong",
        "",
        `${String(i + 1).padStart(2, "0")} · 촬영 그룹 ${s.group + 1} / 카메라 ${s.camera + 1} / 오디오 소스 ${s.audio + 1}`,
      ),
      el("p", "", s.reason),
    );
    selections.append(card);
  });
}
async function loadHistory() {
  try {
    const data = await api("/api/jobs");
    state.jobs = data.jobs;
    $("#job-count").textContent = state.jobs.length;
    const root = $("#history-list");
    root.replaceChildren();
    if (!state.jobs.length)
      root.append(
        el(
          "div",
          "empty-state",
          "아직 만든 쇼츠가 없어요. 첫 번째 이야기를 시작해 보세요.",
        ),
      );
    for (const job of state.jobs) {
      const card = el("article", "panel history-card");
      const info = el("div");
      const date = new Date(Number(job.created)).toLocaleString("ko-KR");
      info.append(
        el("span", "section-label", date),
        el("h3", "", job.title || "인터뷰 쇼츠"),
        el("p", "", job.brief),
      );
      const actions = el("div");
      actions.append(
        el(
          "p",
          "status-label",
          {
            running: "편집 중",
            done: "완료",
            failed: "실패",
            cancelled: "취소됨",
            interrupted: "서버 재시작으로 중단",
          }[job.status] || job.status,
        ),
      );
      const button = el(
        "button",
        "button secondary small",
        job.status === "done" ? "결과 열기" : "상태 확인",
      );
      button.addEventListener("click", () => {
        if (job.status === "done") showResult(job);
        else {
          state.job = job;
          page("create");
          showStep(3);
          renderProgress(job);
          if (job.status === "running") pollJob();
        }
      });
      actions.append(button);
      card.append(info, actions);
      root.append(card);
    }
    const running = state.jobs.find((j) => j.status === "running");
    if (running && !state.job) {
      state.job = running;
      showStep(3);
      renderProgress(running);
      pollJob();
    }
    $("#start-job").disabled =
      !!running ||
      !state.health?.model_ready ||
      !state.health?.speech_available;
  } catch (e) {
    notify(e.message);
  }
}
$$("[data-page]").forEach((n) =>
  n.addEventListener("click", () => {
    page(n.dataset.page);
    if (
      n.dataset.page === "create" &&
      state.step >= 3 &&
      state.job?.status !== "running"
    ) {
      state.job = null;
      $("#result-video").pause();
      $("#result-video").classList.add("hidden");
      $("#preview-empty").classList.remove("hidden");
      showStep(1);
    }
  }),
);
$$("[data-step]").forEach((n) =>
  n.addEventListener("click", () => {
    const s = Number(n.dataset.step);
    if (s === 1 || s === 2) {
      if (state.job?.status === "running") {
        toast("편집 중에는 작업 상태 화면에서 확인해 주세요.");
        return;
      }
      if (s === 2 && mediaProblem()) {
        notify(mediaProblem());
        return;
      }
      showStep(s);
    } else if (s === 3 && state.job) {
      showStep(3);
      renderProgress(state.job);
    } else if (s === 4 && state.job?.status === "done") showResult(state.job);
  }),
);
$("#file-input").addEventListener("change", (e) => {
  addFiles(e.target.files);
  e.target.value = "";
});
const drop = $("#dropzone");
for (const event of ["dragenter", "dragover"])
  drop.addEventListener(event, (e) => {
    e.preventDefault();
    drop.classList.add("dragging");
  });
for (const event of ["dragleave", "drop"])
  drop.addEventListener(event, (e) => {
    e.preventDefault();
    drop.classList.remove("dragging");
  });
drop.addEventListener("drop", (e) => addFiles(e.dataTransfer.files));
$("#add-group").addEventListener("click", () => {
  if (state.groups.length >= 32) {
    notify("최대 32개 촬영 그룹을 만들 수 있습니다.");
    return;
  }
  state.groups.push({
    name: `인터뷰 촬영 ${String(state.groups.length + 1).padStart(2, "0")}`,
    sync: "audio",
  });
  renderGroups();
  saveDraft();
});
$("#to-brief").addEventListener("click", () => {
  const problem = mediaProblem();
  if (problem) {
    notify(problem);
    return;
  }
  showStep(2);
  checkHealth();
});
$("#back-media").addEventListener("click", () => showStep(1));
$("#brief").addEventListener("input", saveDraft);
$("#duration").addEventListener("change", saveDraft);
$("#speech-model").addEventListener("change", saveDraft);
$$("[data-prompt]").forEach((b) =>
  b.addEventListener("click", () => {
    $("#brief").value = b.dataset.prompt;
    saveDraft();
  }),
);
$("#start-job").addEventListener("click", startJob);
$("#cancel-job").addEventListener("click", async () => {
  if (!state.job) return;
  try {
    await api(`/api/jobs/${state.job.id}/cancel`, { method: "POST" });
    state.job = await api(`/api/jobs/${state.job.id}`);
    renderProgress(state.job);
    loadHistory();
    checkHealth();
  } catch (e) {
    notify(e.message);
  }
});
$("#new-job").addEventListener("click", () => {
  state.job = null;
  $("#result-video").pause();
  $("#result-video").removeAttribute("src");
  $("#result-video").classList.add("hidden");
  $("#preview-empty").classList.remove("hidden");
  showStep(1);
  checkHealth();
});
$("#refresh-history").addEventListener("click", loadHistory);
$("#check-health").addEventListener("click", checkHealth);
restoreDraft();
renderGroups();
checkHealth();
loadHistory();
setInterval(() => {
  if (!state.job || state.job.status !== "running") checkHealth();
}, 15000);
