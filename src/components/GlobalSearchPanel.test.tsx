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
    expect(screen.getByPlaceholderText('输入中文关键词，例如：项目资料 / 下一步 / PRD')).toBeTruthy();
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
});
