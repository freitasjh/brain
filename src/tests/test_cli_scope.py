"""Tests for CLI commands with scope parameter."""

import pytest
from unittest.mock import patch, MagicMock
from typer.testing import CliRunner
from brain_server.cli.main import app
from brain_server.cli.mcp_client import MCPClient


runner = CliRunner()


class TestCLIHelpWithScope:
    """Test CLI help text shows scope parameter."""

    def test_store_help_shows_scope(self):
        """Store command help should mention scope parameter."""
        result = runner.invoke(app, ["store", "--help"])
        assert result.exit_code == 0
        assert "--scope" in result.stdout or "-s" in result.stdout

    def test_read_help_shows_scope(self):
        """Read command help should mention scope parameter."""
        result = runner.invoke(app, ["read", "--help"])
        assert result.exit_code == 0
        assert "--scope" in result.stdout or "-s" in result.stdout

    def test_search_help_shows_scope(self):
        """Search command help should mention scope parameter."""
        result = runner.invoke(app, ["search", "--help"])
        assert result.exit_code == 0
        assert "--scope" in result.stdout or "-s" in result.stdout


class TestCLIStoreWithScope:
    """Test store command passes scope correctly to MCP."""

    @patch.object(MCPClient, "call")
    def test_store_passes_scope_to_mcp(self, mock_call):
        """When --scope is provided, it should be included in MCP call args."""
        mock_call.return_value = "ok — regras/global/test.md"
        
        result = runner.invoke(app, [
            "store", "regras", "test/path", "content", "--scope", "global"
        ])
        
        assert result.exit_code == 0
        mock_call.assert_called_once()
        call_args = mock_call.call_args
        assert call_args[0][0] == "brain_store"
        assert call_args[0][1]["scope"] == "global"
        assert call_args[0][1]["layer"] == "regras"
        assert call_args[0][1]["path"] == "test/path"
        assert call_args[0][1]["content"] == "content"

    @patch.object(MCPClient, "call")
    def test_store_omits_scope_when_not_provided(self, mock_call):
        """When --scope is not provided, it should not be in MCP call args."""
        mock_call.return_value = "ok — sessoes/test.md"
        
        result = runner.invoke(app, [
            "store", "sessoes", "test/path", "content"
        ])
        
        assert result.exit_code == 0
        mock_call.assert_called_once()
        call_args = mock_call.call_args
        assert "scope" not in call_args[0][1]


class TestCLIReadWithScope:
    """Test read command passes scope correctly to MCP."""

    @patch.object(MCPClient, "call")
    def test_read_passes_scope_to_mcp(self, mock_call):
        """When --scope is provided, it should be included in MCP call args."""
        mock_call.return_value = "# test\n\ncontent"
        
        result = runner.invoke(app, [
            "read", "regras", "test/path", "--scope", "projetos"
        ])
        
        assert result.exit_code == 0
        mock_call.assert_called_once()
        call_args = mock_call.call_args
        assert call_args[0][0] == "brain_read"
        assert call_args[0][1]["scope"] == "projetos"
        assert call_args[0][1]["layer"] == "regras"
        assert call_args[0][1]["path"] == "test/path"


class TestCLISearchWithScope:
    """Test search command passes scope correctly to MCP."""

    @patch.object(MCPClient, "call")
    def test_search_passes_scope_to_mcp(self, mock_call):
        """When --scope is provided, it should be included in MCP call args."""
        mock_call.return_value = '{"results": [], "total": 0}'
        
        result = runner.invoke(app, [
            "search", "query", "--scope", "global"
        ])
        
        assert result.exit_code == 0
        mock_call.assert_called_once()
        call_args = mock_call.call_args
        assert call_args[0][0] == "brain_search"
        assert call_args[0][1]["scope"] == "global"
        assert call_args[0][1]["query"] == "query"

    @patch.object(MCPClient, "call")
    def test_search_with_layer_and_scope(self, mock_call):
        """Both --layer and --scope can be combined."""
        mock_call.return_value = '{"results": [], "total": 0}'
        
        result = runner.invoke(app, [
            "search", "query", "--layer", "regras", "--scope", "projetos"
        ])
        
        assert result.exit_code == 0
        mock_call.assert_called_once()
        call_args = mock_call.call_args
        assert call_args[0][1]["layer"] == "regras"
        assert call_args[0][1]["scope"] == "projetos"
