"""Test suite for Debug."""

from rattler_build import Debug


class TestDebugCreation:
    """Test suite for Debug creation."""

    def test_create_default(self) -> None:
        """Test creating Debug with default value (disabled)."""
        debug = Debug()
        assert not debug.is_enabled()

    def test_create_enabled(self) -> None:
        """Test creating Debug with enabled=True."""
        debug = Debug(True)
        assert debug.is_enabled()

    def test_create_disabled(self) -> None:
        """Test creating Debug with enabled=False."""
        debug = Debug(False)
        assert not debug.is_enabled()

    def test_factory_enabled(self) -> None:
        """Test Debug.enabled() factory method."""
        debug = Debug.enabled()
        assert debug.is_enabled()

    def test_factory_disabled(self) -> None:
        """Test Debug.disabled() factory method."""
        debug = Debug.disabled()
        assert not debug.is_enabled()


class TestDebugModification:
    """Test suite for modifying Debug state."""

    def test_set_enabled_true(self) -> None:
        """Test setting debug to enabled."""
        debug = Debug(False)
        assert not debug.is_enabled()
        debug.set_enabled(True)
        assert debug.is_enabled()

    def test_set_enabled_false(self) -> None:
        """Test setting debug to disabled."""
        debug = Debug(True)
        assert debug.is_enabled()
        debug.set_enabled(False)
        assert not debug.is_enabled()

    def test_enable(self) -> None:
        """Test enable() method."""
        debug = Debug(False)
        debug.enable()
        assert debug.is_enabled()

    def test_disable(self) -> None:
        """Test disable() method."""
        debug = Debug(True)
        debug.disable()
        assert not debug.is_enabled()

    def test_toggle_from_disabled(self) -> None:
        """Test toggle() from disabled to enabled."""
        debug = Debug(False)
        debug.toggle()
        assert debug.is_enabled()

    def test_toggle_from_enabled(self) -> None:
        """Test toggle() from enabled to disabled."""
        debug = Debug(True)
        debug.toggle()
        assert not debug.is_enabled()

    def test_toggle_twice(self) -> None:
        """Test toggle() twice returns to original state."""
        debug = Debug(True)
        debug.toggle()
        debug.toggle()
        assert debug.is_enabled()


class TestDebugBooleanBehavior:
    """Test suite for boolean behavior of Debug."""

    def test_bool_when_enabled(self) -> None:
        """Test that enabled Debug evaluates to True."""
        debug = Debug(True)
        assert bool(debug)
        assert debug  # Direct boolean check

    def test_bool_when_disabled(self) -> None:
        """Test that disabled Debug evaluates to False."""
        debug = Debug(False)
        assert not bool(debug)
        assert not debug  # Direct boolean check

    def test_if_statement_enabled(self) -> None:
        """Test using Debug in if statement when enabled."""
        debug = Debug(True)
        result = "enabled" if debug else "disabled"
        assert result == "enabled"

    def test_if_statement_disabled(self) -> None:
        """Test using Debug in if statement when disabled."""
        debug = Debug(False)
        result = "enabled" if debug else "disabled"
        assert result == "disabled"


class TestDebugStringRepresentation:
    """Test suite for string representations."""

    def test_repr_enabled(self) -> None:
        """Test __repr__ when debug is enabled."""
        debug = Debug(True)
        repr_str = repr(debug)
        assert "Debug" in repr_str
        assert "True" in repr_str or "enabled=True" in repr_str

    def test_repr_disabled(self) -> None:
        """Test __repr__ when debug is disabled."""
        debug = Debug(False)
        repr_str = repr(debug)
        assert "Debug" in repr_str
        assert "False" in repr_str or "enabled=False" in repr_str

    def test_str_enabled(self) -> None:
        """Test __str__ when debug is enabled."""
        debug = Debug(True)
        str_repr = str(debug)
        assert "enabled" in str_repr.lower()

    def test_str_disabled(self) -> None:
        """Test __str__ when debug is disabled."""
        debug = Debug(False)
        str_repr = str(debug)
        assert "disabled" in str_repr.lower()


class TestDebugIntegration:
    """Integration tests for Debug."""

    def test_workflow_enable_disable(self) -> None:
        """Test a workflow of enabling and disabling debug."""
        # Start disabled
        debug = Debug()
        assert not debug.is_enabled()

        # Enable for debugging
        debug.enable()
        assert debug.is_enabled()

        # Disable after debugging
        debug.disable()
        assert not debug.is_enabled()

    def test_workflow_toggle(self) -> None:
        """Test a workflow using toggle."""
        debug = Debug.disabled()

        # Toggle to enable
        debug.toggle()
        assert debug.is_enabled()

        # Toggle to disable
        debug.toggle()
        assert not debug.is_enabled()

    def test_conditional_workflow(self) -> None:
        """Test using Debug in conditional workflow."""
        debug = Debug(True)

        messages = []
        if debug:
            messages.append("Debug output enabled")

        assert len(messages) == 1
        assert messages[0] == "Debug output enabled"

    def test_state_persistence(self) -> None:
        """Test that Debug state persists correctly."""
        debug = Debug(True)
        assert debug.is_enabled()

        # State should persist
        assert debug.is_enabled()
        assert debug.is_enabled()

        debug.disable()
        assert not debug.is_enabled()
        assert not debug.is_enabled()


class TestDebugEdgeCases:
    """Test edge cases for Debug."""

    def test_multiple_enables(self) -> None:
        """Test that multiple enables don't cause issues."""
        debug = Debug(False)
        debug.enable()
        debug.enable()
        debug.enable()
        assert debug.is_enabled()

    def test_multiple_disables(self) -> None:
        """Test that multiple disables don't cause issues."""
        debug = Debug(True)
        debug.disable()
        debug.disable()
        debug.disable()
        assert not debug.is_enabled()

    def test_set_same_value(self) -> None:
        """Test setting Debug to its current value."""
        debug = Debug(True)
        debug.set_enabled(True)
        assert debug.is_enabled()

        debug = Debug(False)
        debug.set_enabled(False)
        assert not debug.is_enabled()

    def test_many_toggles(self) -> None:
        """Test many consecutive toggles."""
        debug = Debug(False)

        for i in range(100):
            debug.toggle()
            expected = (i + 1) % 2 == 1  # Odd iterations should be enabled
            assert debug.is_enabled() == expected


class TestDebugComparison:
    """Test comparison and equality."""

    def test_boolean_comparison(self) -> None:
        """Test comparing Debug with boolean values."""
        enabled = Debug(True)
        disabled = Debug(False)

        assert enabled  # Truthy
        assert not disabled  # Falsy

    def test_state_comparison(self) -> None:
        """Test comparing Debug states."""
        debug1 = Debug(True)
        debug2 = Debug(True)
        debug3 = Debug(False)

        assert debug1.is_enabled() == debug2.is_enabled()
        assert debug1.is_enabled() != debug3.is_enabled()
