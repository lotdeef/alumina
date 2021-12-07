use core::panic;

use crate::{ast::BuiltinType, common::ArenaAllocatable, ir::*};

pub struct ExpressionBuilder<'ir> {
    ir: &'ir IrCtx<'ir>,
}

impl<'ir> ExpressionBuilder<'ir> {
    pub fn new(ir: &'ir IrCtx<'ir>) -> Self {
        Self { ir }
    }

    pub fn local(&self, id: IrId, typ: TyP<'ir>) -> ExprP<'ir> {
        Expr::lvalue(ExprKind::Local(id), typ).alloc_on(self.ir)
    }

    fn fill_block(
        &self,
        target: &mut Vec<Statement<'ir>>,
        iter: impl IntoIterator<Item = Statement<'ir>>,
    ) -> Result<(), ExprP<'ir>> {
        use ExprKind::*;
        use Statement::*;

        for stmt in iter {
            match stmt {
                Expression(expr) if expr.diverges() => return Err(expr),
                Expression(Expr {
                    kind: Block(stmts, ret),
                    ..
                }) => {
                    self.fill_block(target, stmts.into_iter().cloned())?;
                    target.push(Expression(ret))
                }
                Expression(expr) if expr.pure() => {}
                _ => target.push(stmt),
            }
        }

        Ok(())
    }

    pub fn block(
        &self,
        statements: impl IntoIterator<Item = Statement<'ir>>,
        ret: ExprP<'ir>,
    ) -> ExprP<'ir> {
        let mut merged = Vec::new();

        let ret = match self.fill_block(&mut merged, statements.into_iter()) {
            Ok(()) => ret,
            Err(expr) => expr,
        };

        if merged.is_empty() {
            return ret;
        }

        Expr::rvalue(ExprKind::Block(merged.alloc_on(self.ir), ret), ret.typ).alloc_on(self.ir)
    }

    pub fn diverges(&self, exprs: impl IntoIterator<Item = ExprP<'ir>>) -> ExprP<'ir> {
        let block = self.block(
            exprs.into_iter().map(|expr| Statement::Expression(expr)),
            self.void(),
        );

        // This is a bit of hack, helper function for blocks that diverge. To simplify the caller's code,
        // we accept any block (can contain excess) and if it doesn't actually diverge, we panic here.
        assert!(block.diverges());

        block
    }

    pub fn assign(&self, lhs: ExprP<'ir>, rhs: ExprP<'ir>) -> ExprP<'ir> {
        Expr::rvalue(
            ExprKind::Assign(lhs, rhs),
            self.ir.intern_type(Ty::Builtin(BuiltinType::Void)),
        )
        .alloc_on(self.ir)
    }

    pub fn void(&self) -> ExprP<'ir> {
        Expr::rvalue(
            ExprKind::Void,
            self.ir.intern_type(Ty::Builtin(BuiltinType::Void)),
        )
        .alloc_on(self.ir)
    }

    pub fn function(&self, item: IRItemP<'ir>) -> ExprP<'ir> {
        let func = item.get_function();
        let args: Vec<_> = func.args.iter().map(|arg| arg.ty).collect();
        let ty = Ty::Fn(args.alloc_on(self.ir), func.return_type);

        Expr::const_lvalue(ExprKind::Fn(item), self.ir.intern_type(ty)).alloc_on(self.ir)
    }

    pub fn unreachable(&self) -> ExprP<'ir> {
        Expr::rvalue(
            ExprKind::Unreachable,
            self.ir.intern_type(Ty::Builtin(BuiltinType::Never)),
        )
        .alloc_on(self.ir)
    }

    pub fn tuple_index(&self, tuple: ExprP<'ir>, index: usize, typ: TyP<'ir>) -> ExprP<'ir> {
        let expr = Expr {
            kind: ExprKind::TupleIndex(tuple, index),
            value_type: tuple.value_type,
            is_const: tuple.is_const,
            typ,
        };

        expr.alloc_on(self.ir)
    }

    pub fn deref(&self, inner: ExprP<'ir>) -> ExprP<'ir> {
        let result = match inner.typ {
            Ty::Pointer(ty, false) => Expr::lvalue(ExprKind::Deref(inner), ty),
            Ty::Pointer(ty, true) => Expr::const_lvalue(ExprKind::Deref(inner), ty),
            _ => panic!("not a pointer"),
        };

        result.alloc_on(self.ir)
    }

    pub fn r#ref(&self, inner: ExprP<'ir>) -> ExprP<'ir> {
        assert!(matches!(inner.value_type, ValueType::LValue));

        let result = Expr::rvalue(
            ExprKind::Ref(inner),
            self.ir.intern_type(Ty::Pointer(inner.typ, inner.is_const)),
        );

        result.alloc_on(self.ir)
    }

    pub fn index(&self, inner: ExprP<'ir>, index: ExprP<'ir>) -> ExprP<'ir> {
        let kind = ExprKind::Index(inner, index);
        let result = match inner.typ {
            Ty::Pointer(ty, false) => Expr::lvalue(kind, ty),
            Ty::Pointer(ty, true) => Expr::const_lvalue(kind, ty),
            Ty::Array(ty, _) if !inner.is_const => Expr::lvalue(kind, ty),
            Ty::Array(ty, _) if inner.is_const => Expr::const_lvalue(kind, ty),
            _ => panic!("cannot index"),
        };

        result.alloc_on(self.ir)
    }

    pub fn field(&self, obj: ExprP<'ir>, field_id: IrId, typ: TyP<'ir>) -> ExprP<'ir> {
        let expr = Expr {
            kind: ExprKind::Field(obj, field_id),
            value_type: obj.value_type,
            is_const: obj.is_const,
            typ,
        };

        expr.alloc_on(self.ir)
    }
}
