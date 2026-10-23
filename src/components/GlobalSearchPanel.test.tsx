import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import type { GlobalSearchResult } from '../features/project/desktopApi';
import { formatResultType, GlobalSearchPanel } from './GlobalSearchPanel';

const appState = vi.hoisted(() => ({
  openProject: vi.fn(),
  openFilePath: vi.fn(),
  openFolderPath: vi.fn(),
  setError: vi.fn(),
}));

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(async () => vi.fn()) }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn() }));
vi.mock('../app/AppState', () => ({ useAppState: () => appState }));

const searchResult: GlobalSearchResult = {
  id: 'result-1',
  fileId: '',
  hash: '',
  projectId: 'project-1',
  projectRoot: 'D:\\DemoWorkspace\\ui',
  projectName: '感冒院验收项目',
  contentType: 'project',
  title: '全局搜索验收结果',
  snippet: '搜索结果卡片已显示。',
  updatedAt: '1',
  managedPath: '',
  workspaceRelativePath: '',
  fileType: '',
  ownershipType: '',
  category: '',
  documentType: '',
  documentPurpose: '',
  businessDomain: '',
  lifecycleStatus: '',
  duplicateOf: '',
  versionGroupId: '',
  versionNumber: 1,
  evidenceRefs: [],
  decisionTraceId: '',
  recentStatus: '',
  confidenceDisplay: '',
  matchedField: 'title',
  matchSnippet: '全局搜索',
};

function renderPanel() {
  return render(
    <MemoryRouter>
      <GlobalSearchPanel />
    </MemoryRouter>,
  );
}

describe('GlobalSearchPanel', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === 'search_projects') return [searchResult];
      return [];
    });
  });

  afterEach(() => {
    cleanup();
  });

  it('exposes an accessible name for the global search input', () => {
    renderPanel();

    expect(screen.getByRole('textbox', { name: '全局搜索' })).toBeTruthy();
    expect(screen.getByPlaceholderText('输入关键词，例如：项目资料 / 下一步 / Word 或 PDF 正文条款')).toBeTruthy();
    expect(screen.queryByText(/DeepSeek|Atlas/)).toBeNull();
  });

  it('uses user-facing names for implementation-backed result types', () => {
    expect(formatResultType('atlas')).toBe('项目分析');
    expect(formatResultType('codexReport')).toBe('执行报告');
  });

  it('shows a result card after entering a search term', async () => {
    renderPanel();

    fireEvent.change(screen.getByRole('textbox', { name: '全局搜索' }), {
      target: { value: '验收' },
    });

    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith('search_projects', { query: '验收' });
      expect(screen.getByText('全局搜索验收结果')).toBeTruthy();
    });
  });

  it('keeps the newest query results when an older request resolves later', async () => {
    let resolveOld!: (results: GlobalSearchResult[]) => void;
    let resolveNew!: (results: GlobalSearchResult[]) => void;
    const oldResult = { ...searchResult, id: 'old-result', title: '旧关键词结果' };
    const newResult = { ...searchResult, id: 'new-result', title: '最新关键词结果' };
    const oldRequest = new Promise<GlobalSearchResult[]>((resolve) => { resolveOld = resolve; });
    const newRequest = new Promise<GlobalSearchResult[]>((resolve) => { resolveNew = resolve; });
    vi.mocked(invoke).mockImplementation((command, args) => {
      if (command !== 'search_projects') return Promise.resolve([]);
      const query = (args as unknown as { query?: string } | undefined)?.query;
      return Promise.resolve(query === '旧关键词' ? oldRequest : newRequest);
    });

    renderPanel();
    const input = screen.getByRole('textbox', { name: '全局搜索' });
    fireEvent.change(input, { target: { value: '旧关键词' } });
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('search_projects', { query: '旧关键词' }));

    fireEvent.change(input, { target: { value: '最新关键词' } });
    await waitFor(() => expect(invoke).toHaveBeenCalledWith('search_projects', { query: '最新关键词' }));
    resolveNew([newResult]);
    await waitFor(() => expect(screen.getByText('最新关键词结果')).toBeTruthy());

    resolveOld([oldResult]);
    await waitFor(() => expect(screen.queryByText('旧关键词结果')).toBeNull());
    expect(screen.getByText('最新关键词结果')).toBeTruthy();
  });

  it('shows the matched document body and opens the actual managed file path', async () => {
    appState.openFilePath.mockResolvedValue(undefined);
    const bodyResult: GlobalSearchResult = {
      ...searchResult,
      id: 'file:docx-1',
      fileId: 'docx-1',
      contentType: 'file',
      title: 'document-alpha.docx',
      snippet: '解析摘要：普通项目说明。',
      managedPath: 'D:\\Acceptance\\SearchDemo\\.ganmaoyuan\\managed\\document-alpha.docx',
      workspaceRelativePath: '.ganmaoyuan/managed/document-alpha.docx',
      matchedField: '正文',
      matchSnippet: '...正文条款 WordOnlyClauseX120 ...',
    };
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === 'search_projects') return [bodyResult];
      return [];
    });

    renderPanel();
    fireEvent.change(screen.getByRole('textbox', { name: '全局搜索' }), {
      target: { value: 'WordOnlyClauseX120' },
    });

    await waitFor(() => {
      expect(screen.getByText('document-alpha.docx')).toBeTruthy();
      expect(screen.getByText(/命中 正文：.*WordOnlyClauseX120/)).toBeTruthy();
      expect(screen.getByText(/位置：.*document-alpha\.docx/)).toBeTruthy();
    });
    fireEvent.click(screen.getByRole('button', { name: '打开文件' }));
    await waitFor(() => expect(appState.openFilePath).toHaveBeenCalledWith(bodyResult.managedPath));
  });

  it('renders a body-search budget notice as non-clickable feedback', async () => {
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === 'search_projects') {
        return [{
          ...searchResult,
          id: 'search-notice:document-body-budget',
          contentType: 'searchNotice',
          snippet: '本次有 2 份较大资料正文未检索。',
          projectRoot: '',
          managedPath: '',
        }];
      }
      return [];
    });

    renderPanel();
    fireEvent.change(screen.getByRole('textbox', { name: '全局搜索' }), {
      target: { value: '超大文件条款' },
    });

    await waitFor(() => expect(screen.getByText('本次有 2 份较大资料正文未检索。')).toBeTruthy());
    expect(screen.queryByRole('button', { name: '打开文件' })).toBeNull();
  });
});
