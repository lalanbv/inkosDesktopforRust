import type {TSchema} from '@sinclair/typebox';
import {Value} from '@sinclair/typebox/value';

/**
 * 1017/1018 号：schema 结构的运行时消费面类型——typebox 0.34 的 TSchema 基
 * 接口带有 type/items/properties 等可选字段，1.x（typebox 新包名）把结构
 * 字段收窄到子类（TObject/TArray）。本模块的递归解码只消费这些结构字段，
 * 用结构类型（duck-typing）声明消费面：与 0.x/1.x 双兼容，运行时行为不变
 * （1.4.7 探针实测 properties/items/type 字段仍在产物上）。
 */
export interface SchemaLike {
  readonly type?: string;
  readonly items?: SchemaLike;
  readonly properties?: Record<string, SchemaLike>;
}

/** Decode only valid JSON explicitly supplied in object/array tool fields. Never repair prose. */
export function decodeStructuredFields(schema:TSchema|SchemaLike,value:unknown,paths:string[]=[],path=''):unknown {
  const structural = schema as SchemaLike;
  if(typeof value==='string'&&(structural.type==='object'||structural.type==='array')){
    try{
      const decoded=JSON.parse(value);
      if(Value.Check(schema as TSchema,decoded)){paths.push(path||'/');return decoded;}
    }catch{/* Keep the original value so normal validation reports the error. */}
    return value;
  }
  if(structural.type==='array'&&Array.isArray(value)&&structural.items)return value.map((v,i)=>decodeStructuredFields(structural.items as TSchema,v,paths,`${path}/${i}`));
  if(structural.type==='object'&&value&&typeof value==='object'&&!Array.isArray(value))return Object.fromEntries(Object.entries(value).map(([key,v])=>[key,structural.properties?.[key]?decodeStructuredFields(structural.properties[key] as TSchema,v,paths,`${path}/${key}`):v]));
  return value;
}
