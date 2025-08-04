package co.juandevgo.paleoapi.application.mapper;

import co.juandevgo.paleoapi.application.dto.SpecieDTO;
import co.juandevgo.paleoapi.infrastructure.entity.SpecieEntity;
import org.mapstruct.Mapper;

import java.util.ArrayList;
import java.util.List;

@Mapper(componentModel = "spring")
public interface SpecieMapper {

    SpecieEntity toEntity(SpecieDTO dto);

    SpecieDTO toDTO(SpecieEntity entity);

    default List<SpecieDTO> toDTOList(List<SpecieEntity> entityList) {
        List<SpecieDTO> DTOList = new ArrayList<>();
        if(entityList != null) {
            DTOList.addAll(entityList.stream().map(this::toDTO).toList());
        }
        return DTOList;
    }

    default List<SpecieEntity> toEntityList(List<SpecieDTO> DTOList) {
        List<SpecieEntity> entityList = new ArrayList<>();
        if(DTOList != null) {
            entityList.addAll(DTOList.stream().map(this::toEntity).toList());
        }
        return entityList;
    }
}