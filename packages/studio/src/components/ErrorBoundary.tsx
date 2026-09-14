import { Component } from "react";
import type { ErrorInfo, ReactNode } from "react";

interface ErrorBoundaryProps {
  readonly children: ReactNode;
}

interface ErrorBoundaryState {
  readonly error: Error | null;
}

/**
 * 463 号：顶层崩溃遏制——任一组件渲染错误不再白屏整个应用（449 号走查
 * 实测 root 空挂载形态），降级为受限错误卡 + 重试。挂载于 main.tsx 根部。
 * 刻意不依赖 i18n/app-language 等模块：崩溃源可能是任意 import 链。
 */
export class ErrorBoundary extends Component<ErrorBoundaryProps, ErrorBoundaryState> {
  state: ErrorBoundaryState = { error: null };

  static getDerivedStateFromError(error: Error): ErrorBoundaryState {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    // 保留控制台现场供排障；通知中心等后续面可经 props 注入回调扩展。
    console.error("[InkOS] unhandled render error:", error, info.componentStack);
  }

  render() {
    if (this.state.error) {
      return (
        <div className="flex min-h-screen items-center justify-center p-8" data-testid="error-boundary">
          <div className="max-w-md space-y-3 rounded-xl border border-destructive/30 bg-destructive/5 p-6">
            <h1 className="text-lg font-bold text-destructive">界面遇到了问题</h1>
            <p className="break-all text-sm text-muted-foreground">{this.state.error.message}</p>
            <button
              type="button"
              data-testid="error-boundary-retry"
              onClick={() => this.setState({ error: null })}
              className="rounded-lg bg-primary px-4 py-2 text-xs font-bold text-primary-foreground"
            >
              重试
            </button>
          </div>
        </div>
      );
    }
    return this.props.children;
  }
}
