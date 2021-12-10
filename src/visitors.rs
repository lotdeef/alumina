use tree_sitter::Node;

use crate::ast::AstCtx;
use crate::common::{AluminaErrorKind, ArenaAllocatable, SyntaxError, ToSyntaxError};
use crate::name_resolution::path::{Path, PathSegment};
use crate::name_resolution::scope::{NamedItem, Scope};
use crate::parser::ParseCtx;
use crate::AluminaVisitor;

pub struct ScopedPathVisitor<'ast, 'src> {
    ast: &'ast AstCtx<'ast>,
    code: &'src ParseCtx<'src>,
    scope: Scope<'ast, 'src>, // ast: &'ast AstCtx<'ast>
}

impl<'ast, 'src> ScopedPathVisitor<'ast, 'src> {
    pub fn new(ast: &'ast AstCtx<'ast>, scope: Scope<'ast, 'src>) -> Self {
        Self {
            ast,
            code: scope
                .code()
                .expect("cannot run on scope without parse context"),
            scope,
        }
    }
}

pub trait VisitorExt<'src> {
    type ReturnType;

    fn visit_children(&mut self, node: tree_sitter::Node<'src>) -> Self::ReturnType;

    fn visit_children_by_field(
        &mut self,
        node: tree_sitter::Node<'src>,
        field: &'static str,
    ) -> Self::ReturnType;
}

impl<'src, T, E> VisitorExt<'src> for T
where
    T: AluminaVisitor<'src, ReturnType = Result<(), E>>,
{
    type ReturnType = Result<(), E>;

    fn visit_children(&mut self, node: tree_sitter::Node<'src>) -> Result<(), E> {
        let mut cursor = node.walk();
        for node in node.children(&mut cursor) {
            self.visit(node)?;
        }

        Ok(())
    }

    fn visit_children_by_field(
        &mut self,
        node: tree_sitter::Node<'src>,
        field: &'static str,
    ) -> Result<(), E> {
        let mut cursor = node.walk();
        for node in node.children_by_field_name(field, &mut cursor) {
            self.visit(node)?;
        }

        Ok(())
    }
}

impl<'ast, 'src> AluminaVisitor<'src> for ScopedPathVisitor<'ast, 'src> {
    type ReturnType = Result<Path<'ast>, SyntaxError<'src>>;

    fn visit_crate(&mut self, node: tree_sitter::Node<'src>) -> Self::ReturnType {
        Ok(self
            .scope
            .find_crate()
            .ok_or(AluminaErrorKind::CrateNotAllowed)
            .to_syntax_error(node)?
            .path())
    }

    fn visit_super(&mut self, node: tree_sitter::Node<'src>) -> Self::ReturnType {
        Ok(self
            .scope
            .find_super()
            .ok_or(AluminaErrorKind::SuperNotAllowed)
            .to_syntax_error(node)?
            .path())
    }

    fn visit_identifier(&mut self, node: tree_sitter::Node<'src>) -> Self::ReturnType {
        let name = self.code.node_text(node).alloc_on(self.ast);

        Ok(PathSegment(name).into())
    }

    fn visit_type_identifier(&mut self, node: tree_sitter::Node<'src>) -> Self::ReturnType {
        let name = self.code.node_text(node).alloc_on(self.ast);

        Ok(PathSegment(name).into())
    }

    fn visit_scoped_identifier(&mut self, node: tree_sitter::Node<'src>) -> Self::ReturnType {
        let subpath = match node.child_by_field_name("path") {
            Some(subnode) => self.visit(subnode)?,
            None => Path::root(),
        };

        let name = self
            .code
            .node_text(node.child_by_field_name("name").unwrap())
            .alloc_on(self.ast);

        Ok(subpath.extend(PathSegment(name)))
    }

    fn visit_scoped_type_identifier(&mut self, node: tree_sitter::Node<'src>) -> Self::ReturnType {
        let subpath = self.visit(node.child_by_field_name("path").unwrap())?;
        let name = self
            .code
            .node_text(node.child_by_field_name("name").unwrap())
            .alloc_on(self.ast);

        Ok(subpath.extend(PathSegment(name)))
    }
}

pub struct UseClauseVisitor<'ast, 'src> {
    ast: &'ast AstCtx<'ast>,
    code: &'src ParseCtx<'src>,
    prefix: Path<'ast>,
    scope: Scope<'ast, 'src>,
}

impl<'ast, 'src> UseClauseVisitor<'ast, 'src> {
    pub fn new(ast: &'ast AstCtx<'ast>, scope: Scope<'ast, 'src>) -> Self {
        Self {
            ast,
            prefix: Path::default(),
            code: scope
                .code()
                .expect("cannot run on scope without parse context"),
            scope,
        }
    }

    fn parse_use_path(&mut self, node: Node<'src>) -> Result<Path<'ast>, SyntaxError<'src>> {
        let mut visitor = ScopedPathVisitor::new(self.ast, self.scope.clone());
        visitor.visit(node)
    }
}

impl<'ast, 'src> AluminaVisitor<'src> for UseClauseVisitor<'ast, 'src> {
    type ReturnType = Result<(), SyntaxError<'src>>;

    fn visit_use_as_clause(&mut self, node: Node<'src>) -> Result<(), SyntaxError<'src>> {
        let path = self.parse_use_path(node.child_by_field_name("path").unwrap())?;
        let alias = self
            .code
            .node_text(node.child_by_field_name("alias").unwrap())
            .alloc_on(self.ast);

        self.scope
            .add_item(alias, NamedItem::Alias(self.prefix.join_with(path)))
            .to_syntax_error(node)?;

        Ok(())
    }

    fn visit_use_list(&mut self, node: Node<'src>) -> Result<(), SyntaxError<'src>> {
        self.visit_children_by_field(node, "item")
    }

    fn visit_scoped_use_list(&mut self, node: Node<'src>) -> Result<(), SyntaxError<'src>> {
        let suffix = self.parse_use_path(node.child_by_field_name("path").unwrap())?;
        let new_prefix = self.prefix.join_with(suffix);
        let old_prefix = std::mem::replace(&mut self.prefix, new_prefix);

        self.visit(node.child_by_field_name("list").unwrap())?;
        self.prefix = old_prefix;

        Ok(())
    }

    fn visit_identifier(&mut self, node: Node<'src>) -> Result<(), SyntaxError<'src>> {
        let alias = self.code.node_text(node).alloc_on(self.ast);
        self.scope
            .add_item(
                alias,
                NamedItem::Alias(self.prefix.extend(PathSegment(alias))),
            )
            .to_syntax_error(node)?;

        Ok(())
    }

    fn visit_scoped_identifier(&mut self, node: Node<'src>) -> Result<(), SyntaxError<'src>> {
        let path = self.parse_use_path(node.child_by_field_name("path").unwrap())?;
        let name = self
            .code
            .node_text(node.child_by_field_name("name").unwrap())
            .alloc_on(self.ast);

        self.scope
            .add_item(
                name,
                NamedItem::Alias(self.prefix.join_with(path.extend(PathSegment(name)))),
            )
            .to_syntax_error(node)?;

        Ok(())
    }
}