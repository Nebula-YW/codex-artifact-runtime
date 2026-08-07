// Generated from the Capability Catalog. Do not edit.

export interface CapabilityCallOptions {
  signal?: AbortSignal;
}

export interface CapabilityStream<T> extends AsyncIterable<T> {
  close(): Promise<void>;
}

export type WebBrowserAttachInput = Record<string, unknown>;
export type WebBrowserAttachOutput = { attached: boolean; browser: string; session_id: string };

export type WebBrowserListPagesInput = { session_id: string };
export type WebBrowserListPagesOutput = { pages: Array<{ current: boolean; page_id: string; title: string; url: string }> };

export type WebBrowserOpenPageInput = { session_id: string; url: string };
export type WebBrowserOpenPageOutput = { pages: Array<{ current: boolean; page_id: string; title: string; url: string }> };

export type WebBrowserNavigateInput = { page_id: string; session_id: string; url: string };
export type WebBrowserNavigateOutput = { pages: Array<{ current: boolean; page_id: string; title: string; url: string }> };

export type WebBrowserSnapshotInput = { page_id: string; session_id: string };
export type WebBrowserSnapshotOutput = { snapshot: string };

export type WebBrowserClickInput = { intent: "read" | "low_risk" | "high_risk"; page_id: string; session_id: string; target: string };
export type WebBrowserClickOutput = { result: string };

export type WebBrowserFillInput = { page_id: string; session_id: string; target: string; text: string };
export type WebBrowserFillOutput = { result: string };

export type WebBrowserPressInput = { key: string; page_id: string; session_id: string };
export type WebBrowserPressOutput = { result: string };

export type WebBrowserReadInput = { page_id: string; session_id: string; target: string };
export type WebBrowserReadOutput = { text: string };

export type WebBrowserScrollInput = { delta_x?: number; delta_y: number; page_id: string; session_id: string };
export type WebBrowserScrollOutput = { scrolled: boolean };

export type WebBrowserWaitForInput = { page_id: string; session_id: string; text: string };
export type WebBrowserWaitForOutput = { result: string };

export type WebBrowserClosePageInput = { page_id: string; session_id: string };
export type WebBrowserClosePageOutput = { closed: boolean };

export type WebBrowserScreenshotInput = { page_id: string; session_id: string };
export type WebBrowserScreenshotOutput = { artifact_id: string; path: string; root: string };

export type WebBrowserVideoStartInput = { page_id: string; session_id: string };
export type WebBrowserVideoStartOutput = { artifact_id: string; recording: boolean };

export type WebBrowserVideoStopInput = { session_id: string };
export type WebBrowserVideoStopOutput = { artifact_id: string; path: string; root: string };

export type WebBrowserVideoInspectInput = { artifact_id: string };
export type WebBrowserVideoInspectOutput = { artifact_id: string; container: string; decodable: boolean; distinct_frame_hashes: number; duration_seconds: number; frames_changed: boolean; sampled_frames: number; video_codec: string };

export type WebBrowserDownloadInput = { page_id: string; session_id: string; target: string };
export type WebBrowserDownloadOutput = { artifact_id: string; path: string; root: string; suggested_filename: string };

export type FsReadTextInput = { path: string; root: string };
export type FsReadTextOutput = { bytes: number; content: string; path: string; root: string };

export type FsWriteTextInput = { content: string; path: string; root: string };
export type FsWriteTextOutput = { bytes: number; path: string; root: string };

export type ApprovalGetPolicyInput = Record<string, unknown>;
export type ApprovalGetPolicyOutput = { allow_local_writes: boolean; allow_low_risk_browser_actions: boolean; allowed_hosts: Array<string>; high_risk: string };

export type ApprovalRequestInput = { operation: string };
export type ApprovalRequestOutput = { operation: string; reason: string; status: "manual_required" };

export interface CapabilityClient {
  webBrowser: {
    attach(input: WebBrowserAttachInput, options?: CapabilityCallOptions): Promise<WebBrowserAttachOutput>;
    listPages(input: WebBrowserListPagesInput, options?: CapabilityCallOptions): Promise<WebBrowserListPagesOutput>;
    openPage(input: WebBrowserOpenPageInput, options?: CapabilityCallOptions): Promise<WebBrowserOpenPageOutput>;
    navigate(input: WebBrowserNavigateInput, options?: CapabilityCallOptions): Promise<WebBrowserNavigateOutput>;
    snapshot(input: WebBrowserSnapshotInput, options?: CapabilityCallOptions): Promise<WebBrowserSnapshotOutput>;
    click(input: WebBrowserClickInput, options?: CapabilityCallOptions): Promise<WebBrowserClickOutput>;
    fill(input: WebBrowserFillInput, options?: CapabilityCallOptions): Promise<WebBrowserFillOutput>;
    press(input: WebBrowserPressInput, options?: CapabilityCallOptions): Promise<WebBrowserPressOutput>;
    read(input: WebBrowserReadInput, options?: CapabilityCallOptions): Promise<WebBrowserReadOutput>;
    scroll(input: WebBrowserScrollInput, options?: CapabilityCallOptions): Promise<WebBrowserScrollOutput>;
    waitFor(input: WebBrowserWaitForInput, options?: CapabilityCallOptions): Promise<WebBrowserWaitForOutput>;
    closePage(input: WebBrowserClosePageInput, options?: CapabilityCallOptions): Promise<WebBrowserClosePageOutput>;
    screenshot(input: WebBrowserScreenshotInput, options?: CapabilityCallOptions): Promise<WebBrowserScreenshotOutput>;
    videoStart(input: WebBrowserVideoStartInput, options?: CapabilityCallOptions): Promise<WebBrowserVideoStartOutput>;
    videoStop(input: WebBrowserVideoStopInput, options?: CapabilityCallOptions): Promise<WebBrowserVideoStopOutput>;
    videoInspect(input: WebBrowserVideoInspectInput, options?: CapabilityCallOptions): Promise<WebBrowserVideoInspectOutput>;
    download(input: WebBrowserDownloadInput, options?: CapabilityCallOptions): Promise<WebBrowserDownloadOutput>;
  };
  fs: {
    readText(input: FsReadTextInput, options?: CapabilityCallOptions): Promise<FsReadTextOutput>;
    writeText(input: FsWriteTextInput, options?: CapabilityCallOptions): Promise<FsWriteTextOutput>;
  };
  approval: {
    getPolicy(input: ApprovalGetPolicyInput, options?: CapabilityCallOptions): Promise<ApprovalGetPolicyOutput>;
    request(input: ApprovalRequestInput, options?: CapabilityCallOptions): Promise<ApprovalRequestOutput>;
  };
}

export declare const tools: CapabilityClient;
