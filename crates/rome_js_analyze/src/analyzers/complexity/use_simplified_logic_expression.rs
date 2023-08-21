use crate::JsRuleAction;
use rome_analyze::{context::RuleContext, declare_rule, ActionCategory, Ast, Rule, RuleDiagnostic};
use rome_console::markup;
use rome_diagnostics::Applicability;
use rome_js_factory::make;
use rome_js_syntax::{
    AnyJsExpression, AnyJsLiteralExpression, JsBooleanLiteralExpression, JsLogicalExpression,
    JsUnaryExpression, JsUnaryOperator, T,
};
use rome_rowan::{AstNode, AstNodeExt, BatchMutationExt};

declare_rule! {
    /// Discard redundant terms from logical expressions.
    ///
    /// ## Examples
    ///
    /// ### Invalid
    ///
    /// ```js,expect_diagnostic
    /// const boolExp = true;
    /// const r = true && boolExp;
    /// ```
    ///
    /// ```js,expect_diagnostic
    /// const boolExp2 = true;
    /// const r2 = boolExp || true;
    /// ```
    ///
    /// ```js,expect_diagnostic
    /// const nonNullExp = 123;
    /// const r3 = null ?? nonNullExp;
    /// ```
    ///
    /// ```js,expect_diagnostic
    /// const boolExpr1 = true;
    /// const boolExpr2 = false;
    /// const r4 = !boolExpr1 || !boolExpr2;
    /// ```
    ///
    /// ### Valid
    /// ```js
    /// const boolExpr3 = true;
    /// const boolExpr4 = false;
    /// const r5 = !(boolExpr1 && boolExpr2);
    /// const boolExpr5 = true;
    /// const boolExpr6 = false;
    /// ```
    ///
    pub(crate) UseSimplifiedLogicExpression {
        version: "1.0.0",
        name: "useSimplifiedLogicExpression",
        recommended: false,
    }
}

impl Rule for UseSimplifiedLogicExpression {
    type Query = Ast<JsLogicalExpression>;
    /// First element of tuple is if the expression is simplified by [De Morgan's Law](https://en.wikipedia.org/wiki/De_Morgan%27s_laws) rule, the second element is the expression to replace.
    type State = (bool, AnyJsExpression);
    type Signals = Option<Self::State>;
    type Options = ();

    fn run(ctx: &RuleContext<Self>) -> Option<Self::State> {
        let node = ctx.query();
        let left = node.left().ok()?;
        let right = node.right().ok()?;
        match node.operator().ok()? {
            rome_js_syntax::JsLogicalOperator::NullishCoalescing
                if matches!(
                    left,
                    AnyJsExpression::AnyJsLiteralExpression(
                        AnyJsLiteralExpression::JsNullLiteralExpression(_)
                    )
                ) =>
            {
                return Some((false, right));
            }
            rome_js_syntax::JsLogicalOperator::LogicalOr => {
                if let AnyJsExpression::AnyJsLiteralExpression(
                    AnyJsLiteralExpression::JsBooleanLiteralExpression(literal),
                ) = left
                {
                    return simplify_or_expression(literal, right).map(|expr| (false, expr));
                }

                if let AnyJsExpression::AnyJsLiteralExpression(
                    AnyJsLiteralExpression::JsBooleanLiteralExpression(literal),
                ) = right
                {
                    return simplify_or_expression(literal, left).map(|expr| (false, expr));
                }

                if could_apply_de_morgan(node).unwrap_or(false) {
                    return simplify_de_morgan(node)
                        .map(|expr| (true, AnyJsExpression::JsUnaryExpression(expr)));
                }
            }
            rome_js_syntax::JsLogicalOperator::LogicalAnd => {
                if let AnyJsExpression::AnyJsLiteralExpression(
                    AnyJsLiteralExpression::JsBooleanLiteralExpression(literal),
                ) = left
                {
                    return simplify_and_expression(literal, right).map(|expr| (false, expr));
                }

                if let AnyJsExpression::AnyJsLiteralExpression(
                    AnyJsLiteralExpression::JsBooleanLiteralExpression(literal),
                ) = right
                {
                    return simplify_and_expression(literal, left).map(|expr| (false, expr));
                }

                if could_apply_de_morgan(node).unwrap_or(false) {
                    return simplify_de_morgan(node)
                        .map(|expr| (true, AnyJsExpression::JsUnaryExpression(expr)));
                }
            }
            _ => return None,
        }

        None
    }

    fn diagnostic(ctx: &RuleContext<Self>, _: &Self::State) -> Option<RuleDiagnostic> {
        let node = ctx.query();

        Some(RuleDiagnostic::new(
            rule_category!(),
            node.range(),
            markup! {
                "Logical expression contains unnecessary complexity."
            },
        ))
    }

    fn action(ctx: &RuleContext<Self>, state: &Self::State) -> Option<JsRuleAction> {
        let node = ctx.query();
        let mut mutation = ctx.root().begin();

        let (is_simplified_by_de_morgan, expr) = state;

        mutation.replace_node(
            AnyJsExpression::JsLogicalExpression(node.clone()),
            expr.clone(),
        );

        let message = if *is_simplified_by_de_morgan {
            "Reduce the complexity of the logical expression."
        } else {
            "Discard redundant terms from the logical expression."
        };

        Some(JsRuleAction {
            category: ActionCategory::QuickFix,
            applicability: Applicability::MaybeIncorrect,
            message: markup! { ""{message}"" }.to_owned(),
            mutation,
        })
    }
}

/// https://en.wikipedia.org/wiki/De_Morgan%27s_laws
fn could_apply_de_morgan(node: &JsLogicalExpression) -> Option<bool> {
    let left = node.left().ok()?;
    let right = node.right().ok()?;
    match (left, right) {
        (AnyJsExpression::JsUnaryExpression(left), AnyJsExpression::JsUnaryExpression(right)) => {
            Some(
                matches!(left.operator().ok()?, JsUnaryOperator::LogicalNot)
                    && matches!(right.operator().ok()?, JsUnaryOperator::LogicalNot)
                    && !matches!(left.argument().ok()?, AnyJsExpression::JsUnaryExpression(_))
                    && !matches!(
                        right.argument().ok()?,
                        AnyJsExpression::JsUnaryExpression(_)
                    ),
            )
        }
        _ => Some(false),
    }
}

fn simplify_and_expression(
    literal: JsBooleanLiteralExpression,
    expression: AnyJsExpression,
) -> Option<AnyJsExpression> {
    keep_expression_if_literal(literal, expression, true)
}

fn simplify_or_expression(
    literal: JsBooleanLiteralExpression,
    expression: AnyJsExpression,
) -> Option<AnyJsExpression> {
    keep_expression_if_literal(literal, expression, false)
}

fn keep_expression_if_literal(
    literal: JsBooleanLiteralExpression,
    expression: AnyJsExpression,
    expected_value: bool,
) -> Option<AnyJsExpression> {
    let eval_value = match literal.value_token().ok()?.kind() {
        T![true] => true,
        T![false] => false,
        _ => return None,
    };
    if eval_value == expected_value {
        Some(expression)
    } else {
        Some(AnyJsExpression::AnyJsLiteralExpression(
            AnyJsLiteralExpression::JsBooleanLiteralExpression(literal),
        ))
    }
}

fn simplify_de_morgan(node: &JsLogicalExpression) -> Option<JsUnaryExpression> {
    let left = node.left().ok()?;
    let right = node.right().ok()?;
    let operator_token = node.operator_token().ok()?;
    match (left, right) {
        (AnyJsExpression::JsUnaryExpression(left), AnyJsExpression::JsUnaryExpression(right)) => {
            let mut next_logic_expression = match operator_token.kind() {
                T![||] => node
                    .clone()
                    .replace_token(operator_token, make::token(T![&&])),
                T![&&] => node
                    .clone()
                    .replace_token(operator_token, make::token(T![||])),
                _ => return None,
            }?;
            next_logic_expression = next_logic_expression.with_left(left.argument().ok()?);
            next_logic_expression = next_logic_expression.with_right(right.argument().ok()?);
            Some(make::js_unary_expression(
                make::token(T![!]),
                AnyJsExpression::JsParenthesizedExpression(make::js_parenthesized_expression(
                    make::token(T!['(']),
                    AnyJsExpression::JsLogicalExpression(next_logic_expression),
                    make::token(T![')']),
                )),
            ))
        }
        _ => None,
    }
}
